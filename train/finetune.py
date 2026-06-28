#!/usr/bin/env python3
"""Weight-training scaffold for ncx-forge (the GPU step).

Turns the trajectories that `export.py` produced into actual fine-tuning. This
file is authored to run on a GPU box (it does NOT run here — no GPU); the
data-shaping and reward logic ARE pure Python and unit-tested in this repo, so
only the `.train()` call needs the GPU deps (transformers + trl + torch + peft).

Two paths:

  SFT (supervised imitation) — the primary, clean path:
    Take reward==1 trajectories (verified successes), render each as a chat
    sequence [system_prompt, user(task), assistant/tool turns...], and fine-tune
    the model to reproduce them. The evolved genome's system_prompt is the
    system message, so the model internalises the harness ncx-forge discovered.
      python train/finetune.py --mode sft --data train/data/sft.jsonl --model <hf-model>

  RL (GRPO/PPO) — the reward is `bench pass`:
    `bench_reward()` below is the reusable, verifiable reward (run the hidden
    check, 0/1). NOTE (honest): our agent is a MULTI-TURN, tool-using loop, not a
    single-completion generator, so a vanilla trl GRPOTrainer does not fit
    directly — RL here needs an *agentic* rollout (model turn -> tools execute ->
    repeat -> reward at episode end). The reward fn + the bench harness are the
    reusable pieces; the rollout loop is the work a GPU-side agentic-RL setup adds.
    See `rl_design()` for the contract.

Prereqs on the GPU box: pip install "trl>=0.9" transformers torch peft datasets
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402  (bench.grade reuse for the RL reward)


# ── SFT data shaping (pure, runs + tested here) ─────────────────────────────────

def load_records(path: Path, reward_pass_only: bool = True) -> list[dict]:
    """Read an export.py JSONL; optionally keep only reward==1 (SFT imitation)."""
    out = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        rec = json.loads(line)
        if reward_pass_only and rec.get("reward") != 1:
            continue
        out.append(rec)
    return out


def to_chat(record: dict) -> dict:
    """One export record -> a chat-format SFT example: {"messages": [...]}.

    Prepends the genome's system_prompt as the system message; keeps the agent's
    user/assistant/tool turns (the tool calls ARE the skill being taught). Drops
    empty-content non-assistant turns and any stray system messages in the body.
    """
    msgs = [{"role": "system", "content": record.get("system_prompt", "")}]
    for m in record.get("messages", []):
        role = m.get("role")
        if role == "system":
            continue  # the system prompt is supplied once, above
        msg = {"role": role, "content": m.get("content", "")}
        if m.get("tool_calls"):
            msg["tool_calls"] = m["tool_calls"]
        msgs.append(msg)
    return {"messages": msgs, "task": record.get("task"), "reward": record.get("reward")}


def build_sft_dataset(paths: list[Path], reward_pass_only: bool = True) -> list[dict]:
    """Load + convert one or more export JSONLs into SFT chat examples."""
    examples = []
    for p in paths:
        for rec in load_records(p, reward_pass_only):
            examples.append(to_chat(rec))
    return examples


# ── RL reward (pure, reusable) ──────────────────────────────────────────────────

def bench_reward(task_name: str, workspace: Path) -> float:
    """Verifiable reward for RL: 1.0 if the agent's work in `workspace` passes the
    task's hidden check, else 0.0. Reuses the exact bench grader. This is the
    reward signal for any agentic-RL rollout (GRPO/PPO)."""
    task = next((t for t in ev.bench.tasks() if t.name == task_name), None)
    if task is None:
        return 0.0
    ok, _ = ev.bench.grade(task, Path(workspace))
    return 1.0 if ok else 0.0


def rl_design() -> str:
    return (
        "Agentic GRPO contract:\n"
        "  episode = run the agent (ncx) on a task in a fresh workspace with the\n"
        "  candidate model + the evolved system_prompt; reward = bench_reward(task, ws)\n"
        "  at episode end (0/1). Group N episodes per task for GRPO advantage.\n"
        "  The model is updated on its OWN generated turns (not export traces).\n"
        "  Needs an agentic rollout collector (model<->tools loop) feeding trl GRPO."
    )


# ── GPU-only entry points (lazy imports so this file loads without trl/torch) ───

def run_sft(data_paths: list[Path], model: str, out_dir: str,
            reward_pass_only: bool = True, **kw) -> None:  # pragma: no cover (GPU)
    from datasets import Dataset
    from trl import SFTConfig, SFTTrainer
    from transformers import AutoTokenizer

    examples = build_sft_dataset(data_paths, reward_pass_only)
    if not examples:
        raise SystemExit("no SFT examples (need reward==1 trajectories from export.py)")
    tok = AutoTokenizer.from_pretrained(model)
    ds = Dataset.from_list(examples)

    def fmt(ex):
        return {"text": tok.apply_chat_template(ex["messages"], tokenize=False)}

    ds = ds.map(fmt)
    cfg = SFTConfig(output_dir=out_dir, num_train_epochs=kw.get("epochs", 2),
                    per_device_train_batch_size=kw.get("batch", 1),
                    learning_rate=kw.get("lr", 1e-5), packing=False)
    SFTTrainer(model=model, args=cfg, train_dataset=ds).train()
    print(f"[finetune] SFT done -> {out_dir} ({len(examples)} examples)")


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser(description="ncx-forge weight-training scaffold (GPU).")
    ap.add_argument("--mode", choices=["sft", "grpo", "prep"], default="prep")
    ap.add_argument("--data", nargs="*", default=[], help="export.py JSONL file(s)")
    ap.add_argument("--model", default="", help="HF model id / path to fine-tune")
    ap.add_argument("--out", default="train/finetuned")
    ap.add_argument("--all-rewards", action="store_true", help="keep reward==0 too")
    a = ap.parse_args()
    paths = [Path(p) for p in a.data]

    if a.mode == "prep":
        # No GPU needed: report what the export data would yield for SFT.
        ex = build_sft_dataset(paths, reward_pass_only=not a.all_rewards) if paths else []
        print(f"[finetune] {len(ex)} SFT example(s) from {[str(p) for p in paths]}")
        if ex:
            roles = [m["role"] for m in ex[0]["messages"]]
            print(f"[finetune] example[0]: task={ex[0]['task']} roles={roles}")
        print("\n[finetune] RL design:\n" + rl_design())
        print("\nTo train (on a GPU box):\n"
              "  pip install 'trl>=0.9' transformers torch peft datasets\n"
              "  python train/finetune.py --mode sft --data train/data/sft.jsonl --model <hf-model>")
        return 0
    if a.mode == "sft":
        if not a.model or not paths:
            raise SystemExit("--mode sft needs --model and --data")
        run_sft(paths, a.model, a.out, reward_pass_only=not a.all_rewards)
        return 0
    # The agentic rollout collector rl_design() called for now exists:
    import rollout as R
    print("grpo: agentic RL. The rollout collector is train/rollout.py.")
    print(rl_design())
    print("\nReady pieces: rollout.collect_group() (episodes + GRPO advantages), "
          "rollout.bench_reward (terminal reward), rollout.ncx_episode (reuse ncx's "
          "loop via a vLLM-served policy). Wire rollout.run_grpo's policy_update on a GPU.")
    print("advantage demo:", [round(a, 2) for a in R.grpo_advantages([1.0, 1.0, 0.0, 0.0])])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

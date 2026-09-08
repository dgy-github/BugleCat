"""Render an understandable, reproducible Chinese repository report."""
import argparse
import json
from pathlib import Path

LABELS = {"passed": "通过", "test_failed": "回归测试未通过", "grader_error": "验收环境错误",
          "compile_failed": "候选代码编译失败",
          "grader_timeout": "验收超时", "integrity_failed": "完整性检查失败",
          "infrastructure_error": "基础设施异常"}
FAULTS = {
    "s1_retry": "故意让缺失文件抢占编译失败分类，并丢失重试证据和前序调用耗时。",
    "s2_patch": "故意丢弃补丁的普通上下文，使插入失去定位、重复行可能改错位置。",
    "s3_budget": "故意把部分预算耗尽、取消和验证失败状态当成退出成功。",
    "s4_compile": "故意让 Python 编译错误和未完成的验证不再阻止任务结束。",
    "s5_encoding": "故意用大端解码器读取小端 UTF-16 BOM，复现中文乱码。",
    "s6_diagnostics": "故意遗漏模型字段中的凭据脱敏，并把第一次请求也计为一次重试。",
}


def metrics(state):
    cases = state["cases"]
    first = [c["attempts"][0] for c in cases if c.get("attempts")]
    final = [c["attempts"][-1] for c in cases if c.get("attempts")]
    passed = lambda a: a["grader"]["status"] == "passed"  # noqa: E731
    return dict(total=len(cases), first_pass=sum(map(passed, first)),
                final_pass=sum(map(passed, final)),
                clean_first=sum(passed(a) and a["returncode"] == 0 and not a["timed_out"] for a in first),
                clean_final=sum(passed(a) and a["returncode"] == 0 and not a["timed_out"] for a in final),
                invalid=sum(c["status"].startswith("invalid_") for c in cases),
                infrastructure=sum(c["status"] in ("infrastructure_error", "grader_error", "grader_timeout")
                                   for c in cases),
                invocations=sum(len(c.get("attempts", [])) for c in cases),
                agent_seconds=round(sum(a["elapsed_s"] for c in cases for a in c.get("attempts", [])), 2))


def validate_recheck(state, checked):
    """Do not publish a replay claim based only on the initial run's scores."""
    if checked.get("run_id") != state.get("run_id"):
        raise ValueError("Missing or mismatched independent recheck")
    rows = checked.get("checks", [])
    if (len(rows) != len(state["cases"]) or
            {r["id"] for r in rows} != {c["id"] for c in state["cases"]}):
        raise ValueError("Independent recheck does not cover every case")
    for case in state["cases"]:
        row = next(r for r in rows if r["id"] == case["id"])
        attempts = case.get("attempts", [])
        if not attempts:
            if (not case["status"].startswith("invalid_") or
                    row.get("skipped_reason") != "no_model_attempts"):
                raise ValueError("Unexplained missing model attempts")
            continue
        if (row.get("integrity_issues") or row.get("healthy", {}).get("status") != "passed" or
                row.get("seed", {}).get("status") != "test_failed"):
            raise ValueError("Healthy/seed/integrity recheck failed")
        replayed = row.get("replayed_attempts", [])
        if (len(replayed) != len(attempts) or
                {a["attempt"] for a in replayed} != {a["attempt"] for a in attempts}):
            raise ValueError("Independent recheck does not cover every attempt")
        for attempt in attempts:
            replay = next(a for a in replayed if a["attempt"] == attempt["attempt"])
            expected = attempt["grader"]["status"]
            if (not replay.get("agrees") or replay.get("integrity_issues") or
                    replay.get("original_status") != expected or
                    replay.get("validation", {}).get("status") != expected):
                raise ValueError("Independent replay disagrees with original result")
        if row.get("final", {}).get("status") != attempts[-1]["grader"]["status"]:
            raise ValueError("Final candidate does not match recorded result")


def render(state, checked):
    validate_recheck(state, checked)
    m = metrics(state)
    run = state["run_id"]
    evidence = f"../../evals/self_repair/results/{run}"
    lines = ["# BugleCat 六类自我修复测试报告", "",
             f"运行标识：`{run}`（UTC）。模型：`{state['model']}`。", "",
             "## 一句话结论", "",
             f"在 {m['total']} 道真实源码故障注入题中，首轮补丁通过 {m['first_pass']} 道；"
             f"最多一次反馈后累计通过 {m['final_pass']} 道。首轮同时满足验收通过、正常完成且未超时的有 "
             f"{m['clean_first']} 道。无效基线/注入 {m['invalid']} 道；"
             f"无法可靠验收的基础设施异常 {m['infrastructure']} 道。", "",
             f"最终通过且正常结束的共有 {m['clean_final']} 道；这与只留下可用补丁、进程却超时或失败的情况分开统计。", "",
             "这里的“通过”由独立测试判定，不采用模型自述，也不把进程退出 0 当作修复正确。"
             "每类只运行一道、一次初始尝试，且指定目标文件、提供功能约束和可见测试；"
             "这不是无提示的全仓库诊断，样本不足以代表稳定成功率或所有代码修复能力。", "",
             "## 怎么测：先证明题目真的坏了", "",
             "1. 对健康源码运行真实回归测试，必须通过。",
             "   还会模拟 Agent 的精简 shell 环境，验证冻结测试入口能运行；缺失工具链时不进入模型修复。",
             "2. 在独立工作副本注入可复现缺陷，测试必须出现断言失败；编译环境故障不算有效题目。",
             "3. 只在故障注入后创建单提交 Git 历史，不导出健康历史、历史答案或本套验收器。",
             "4. BugleCat 编辑真实所属文件；外部检查改动范围、既有测试完整性，再独立运行验收。",
             "5. 首轮失败可收到一次测试反例反馈，不能由外部助手代写候选修复。", "",
             "这是合作式开发自测，不是防作弊竞赛沙箱：workspace-write 不阻止读取宿主其他目录，"
             "共享编译缓存也不是隔离边界。未导出答案不等于操作系统强制禁止访问答案。"
             "Rust 验收使用可见但冻结的真实回归测试；Python 分类题另有外部验收。", "",
             "## 六类题目与结果", ""]
    for c in state["cases"]:
        attempts = c.get("attempts", [])
        first = attempts[0]["grader"]["status"] if attempts else "未执行"
        final = attempts[-1]["grader"]["status"] if attempts else c["status"]
        lines += [f"### {c['id']}：{c['title']}", "",
                  FAULTS.get(c["id"], ""), "",
                  f"修复对象：`{c['target']}`。健康基线：{LABELS.get(c['healthy']['status'], c['healthy']['status'])}；"
                  f"注入后：{LABELS.get(c.get('seed', {}).get('status'), '未执行')}。",
                  f"首轮：**{LABELS.get(first, first)}**；最终：**{LABELS.get(final, final)}**。", ""]
        for a in attempts:
            g = a["grader"]
            lines += [f"- 第 {a['attempt']} 次：Agent {a['elapsed_s']} 秒，退出码 {a['returncode']}，"
                      f"外层超时：{'是' if a['timed_out'] else '否'}；验收 {g['passed']} 通过 / {g['failed']} 失败，"
                      f"验收耗时 {g['elapsed_s']} 秒。"
                      f"[候选补丁]({evidence}/{a['patch']})。"]
        lines += [""]
    lines += ["## 指标怎么读", "",
              "- 首轮通过：不给反例反馈时，第一次生成的补丁满足独立验收。",
              "- 反馈后通过：允许同一工作副本再修一次，不能与独立首轮成功率混算。",
              "- 正常完成：验收通过之外，CLI 退出 0 且没有触发外层超时；与补丁正确性分别统计。",
              "- 无效题：健康基线失败、注入后未复现测试失败或环境无法运行，不能冒充模型修复失败。",
              "- 基础设施异常：验收启动失败、验收超时等单列，不能当作已确认的功能错误。",
              f"- 本批 CLI 实际启动 {m['invocations']} 次，Agent 累计 {m['agent_seconds']} 秒"
              f"（约 {m['agent_seconds'] / 60:.1f} 分钟）；"
              "独立验收/编译时间另计，不混入 Agent 时长。", "",
              "上列验收耗时属于原运行；最终清理缓存后重新编译复验的耗时另记于 independent_recheck.json。", "",
              "每次 Agent 总预算 600 秒，外层回收 630 秒，最多 30 次模型调用/80 次工具调用；"
              "HTTP 调用沿用运行时的有限重试策略。独立验收每次最多 600 秒。没有定时任务。", "",
              "## 可复现性与证据", "",
              f"- 起始 Git HEAD：`{state['source_head']}`；健康源码包含该 HEAD 上的本次待提交修复，"
              "不是仅由这个旧提交号定义。各题健康目标 SHA、故障种子 SHA 和结果见证据。",
              f"- 执行器 SHA-256：`{state['binary_sha256']}`。",
              f"- [结构化结果与验收日志]({evidence}/summary.json)；目录内保留每次候选补丁和脱敏调用诊断。",
              f"- [运行后独立复验]({evidence}/independent_recheck.json)：从原始故障 Git 种子逐次重放已保存补丁，"
              "确认原有测试仍实际执行，且最后保存的补丁与实际候选一致。",
              "- 原始会话保留在本机候选副本，不上传凭据、原始模型/工具会话或二进制。",
              "- 独立复验会清理四个可变包的 dev 构建产物后重新编译健康代码、原始故障种子和逐次补丁，"
              "防止共享 Cargo 目录复用其他副本的旧产物；不清理源码或 release 执行器。",
              "- 使用稳定 release CLI 修复源码副本，再通过测试验证源码；不是热更新运行中的 CLI。",
              "- 合成故障候选不会自动覆盖主项目。上一轮 9/9 验收通过的 benchmark 修复与其 12 项测试已独立合入。", "",
              "复跑（需要已配置 DeepSeek Pro 和已构建 release CLI；目录必须不存在）：", "",
              "```powershell",
              "python -m evals.self_repair.suite --work-root D:/BugleCat-selftest-new",
              "# 只验证题目，不调用模型：增加 --validate-only",
              "# 单题：增加 --cases s2_patch（精确 ID，不用子串）", "```", "",
              "本报告不宣称重跑了全量 18 题 benchmark，也不构成 GUI、跨平台或线上可靠性认证。", ""]
    if state.get("preflight_history"):
        lines += ["## 预检异常没有被掩盖", "",
                  f"本批保留了 {len(state['preflight_history'])} 次未调用模型的预检失败记录。"
                  "修正评测环境后仅补跑这些未进入修复阶段的题目；已有模型尝试不重置，不用重跑覆盖失败。"
                  "原始预检结果保存在 summary.json 的 preflight_history 中。", ""]
    return "\n".join(lines)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("summary", type=Path)
    p.add_argument("output", type=Path)
    args = p.parse_args()
    state = json.loads(args.summary.read_text(encoding="utf-8"))
    if state.get("validate_only"):
        raise SystemExit("Validation-only runs cannot produce a model self-repair report")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    checked = json.loads(args.summary.with_name("independent_recheck.json").read_text(encoding="utf-8"))
    args.output.write_text(render(state, checked), encoding="utf-8")


if __name__ == "__main__":
    main()

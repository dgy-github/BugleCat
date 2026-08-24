<script lang="ts">
  type Approval = { session_id: string; id: number; command: string; reason: string; cwd: string; details: string };
  type UserQuestion = { session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean };

  let { approval, userQuestion, questionAnswer = $bindable(), decide, answerUserQuestion }: {
    approval: Approval | null;
    userQuestion: UserQuestion | null;
    questionAnswer: string;
    decide: (choice: "deny" | "always" | "once") => void;
    answerUserQuestion: (answer: string | null) => void;
  } = $props();
</script>

{#if approval}
  <div class="overlay">
    <div class="modal">
      <h3>需要审批</h3>
      <p class="areason">{approval.reason}</p>
      <div class="afield"><span>操作</span><code>{approval.command}</code></div>
      <div class="afield"><span>目录</span><code>{approval.cwd}</code></div>
      {#if approval.details}<pre class="adetails">{approval.details}</pre>{/if}
      <div class="abtns">
        <button class="deny" onclick={() => decide("deny")}>拒绝</button>
        <button class="plain" onclick={() => decide("always")} title="本次会话始终允许（命令 / 编辑）">始终允许</button>
        <button class="ok" onclick={() => decide("once")}>批准</button>
      </div>
    </div>
  </div>
{/if}

{#if userQuestion}
  <div class="overlay">
    <div class="modal question-modal">
      <h3>需要你的选择</h3>
      <p class="areason">{userQuestion.question}</p>
      {#if userQuestion.options.length > 0}
        <div class="question-options">{#each userQuestion.options as option}<button class="plain" onclick={() => answerUserQuestion(option)}>{option}</button>{/each}</div>
      {/if}
      {#if userQuestion.allow_free_text}<textarea bind:value={questionAnswer} rows="3" placeholder="输入你的回答"></textarea>{/if}
      <div class="abtns">
        <button class="deny" onclick={() => answerUserQuestion(null)}>取消</button>
        {#if userQuestion.allow_free_text}<button class="ok" disabled={questionAnswer.trim() === ""} onclick={() => answerUserQuestion(questionAnswer)}>提交</button>{/if}
      </div>
    </div>
  </div>
{/if}

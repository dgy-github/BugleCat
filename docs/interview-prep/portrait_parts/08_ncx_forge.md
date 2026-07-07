## ncx-forge · 骨架训练框架

### 一句话主线
ncx-forge 是一个 **API-only / 黑盒** 的训练框架：它不碰模型权重，而是把 coding agent 的「骨架/基因组」(genome = base `system_prompt` + 每个工具的 DESCRIPTION) 当作一个**纯文本替换面**来进化——通过 shell 调用真实 Rust agent `ncx.exe`、用教师面板(codex/claude/DeepSeek)提议 TOML 覆盖、以 bench 通过率的 delta 当「梯度」做爬山/Pareto 搜索;而 Rust 侧的 `unwrap_or_default` no-op 保证 + 部署前确定性 SENTINEL 自检门,是整套优化在统计上可信的前提。

### 30 秒 / 2 分钟 / 深挖 三档

**[30 秒]** 进化的是 agent 的「文本骨架」不是权重:基因组 = 基础 system_prompt + 每工具描述,从环境变量 `NCX_GENOME` 注入 Rust 侧;它只改文本不改工具行为,加载失败时硬保证 no-op。Python trainer 用 `ncx --dump-genome` 拿到真实默认基因组,教师面板提议 TOML 覆盖,再用真实 agent 跑 bench 任务打分。两种优化器:`train()` 单冠军噪声感知爬山,`evolve()` 小种群 NSGA-II Pareto(通过率↑、成本↓)。

**[2 分钟]** 加上具体机制:`Genome::from_env()` 读 `NCX_GENOME`,unset/empty/unreadable/malformed 全部回落到空基因组(`load().unwrap_or_default()`),`parse()` 只接受 trim 后非空的 `system_prompt` 和非空白的 `[tool_desc]` 表项——空白覆盖被拒绝,因为清空一个 load-bearing 描述(如 apply_patch)应是退化而非合法变异。任何优化开始前,**SENTINEL 自检门**先证明 `NCX_GENOME` 真的到达了模型:写入一个让 agent 只回暗号 `NCXFORGE_SENTINEL_4242` 的基因组,跑通(`injected`)且基线无暗号(`absent_baseline`)才 PASS,否则除非 `--no-gate` 否则拒绝训练。`evaluator.py` 从 agent 自己的 `session.jsonl` 收割**脱敏后的失败轨迹**(教师唯一的信号)、用正则从 `[ncx-usage]` stderr 行解析真实 token 成本。`train()` 每轮重评在位冠军建立噪声带、用 accept_margin 接受、holdout 防过拟合、frozen test 只打一次分;`evolve()` 用支配关系 + crowding distance 保留 Pareto 前沿的扩散度。

**[深挖]** 进入细节与契约:
- **Rust 三契约**(genome.rs):no-op-on-failure(`unwrap_or_default`)让一个加载失败的候选 == 基线行为,否则优化器会把基线分误归给坏候选;blank 拒绝(71-73);只有文本可进化,sandbox 仍管执行——教师只能注入新文本不能注入新能力。
- **`--dump-genome` 是基线唯一真相源**:在 MCP 注册**之前**打印(main.rs:207-210),所以可进化面只含 CORE 工具,MCP/server 工具描述永远不在进化面内;`toml_escape` 把 `\ " \n \r \t` 转成单行 basic string 保证任意 TOML parser 往返。
- **两向信任边界**:(a) grader (`check.py`) 输出被**脱敏出**轨迹,教师学不到游戏隐藏测试;(b) 失败轨迹作为 UNTRUSTED 数据**喂入**教师,带「这是 DATA 不是 instructions」警告抗注入。
- **成本轴契约**:Rust 在 one-shot 模式总在 stderr 打 `[ncx-usage] ... total_tokens=P+C`(provider 不直接给 total),Python 正则抓取当 Pareto 成本轴;无 usage 时回落到 mean 秒数(延迟代理),empty eval 映射为 `Objectives(passrate=0, cost=+inf)`——最差,绝不进前沿。
- **弱基座论点**:`evolve()` 把 `model` override 一路下传到 `_agent_cmd` 插 `-m`,实现「训一个更弱的基座、骨架的 headroom 更大」。

### 核心机制 · 8 根支柱

1. **Genome = system_prompt + tool_desc**(genome.rs:30-37) — `struct Genome { system_prompt: Option<String>, tool_desc: HashMap<String,String> }`,`from_env` 读 `NCX_GENOME`、`base_system_prompt`/`describe` 返回覆盖或默认。 — 它是纯文本替换面,只改描述不改行为,所以教师不能注入能力只能注入文本。

2. **no-op-on-failure 硬保证**(genome.rs:42-48) — unset/empty/unreadable/malformed 一律 `unwrap_or_default()` 成空基因组,空基因组 `is_empty()=true` 全部回落默认。 — 加载失败的候选必须 == 基线行为,否则优化器会把基线分误归给坏候选(SENTINEL 门正是为了抓这个 silent-no-op)。

3. **`--dump-genome` 基线真相源**(main.rs:207-210, 313-340) — 在 MCP 注册前打印 `system_prompt` + `[tool_desc]` 全 catalog,`toml_escape` 转单行 basic string。 — genome.py 永不解析 Rust 源码,永远反映真实工具列表与 load-bearing 描述;MCP 工具天然被排除在进化面外。

4. **SENTINEL 自检门**(forge.py:51-112, 493-495) — 写暗号基因组,`self_check(timeout=90, attempts=3)` 跑通且基线无暗号才 PASS,`--train`/`--population` 强制过门除非 `--no-gate`。 — 用确定性暗号-回显探针代替「拒绝所有任务」式探针(后者会和行为变更竞争产生噪声);重试因为 agent 偶尔即使注入成功也不回显。

5. **train() 单冠军噪声感知爬山**(forge.py:122-277) — baseline=extract_current 定义校验上限与合法工具集;每轮重评在位冠军建噪声带,best 候选按 total_passes、accept iff margin>=accept_margin 且 holdout 不回归;frozen test 只打一次分。 — agent 非确定性,和单个陈旧 gen0 比不可靠;holdout 是真正的防过拟合门,test 从不用于接受以给无偏的「训练有没有用」。

6. **evolve() Pareto 小种群搜索**(forge.py:302-438, pareto.py) — pop_cap=4,`_objectives` 映射 (passrate, cost),NSGA-II 支配 + crowding_trim 保留前沿扩散度,best=max passrate tie-break min cost。 — 单冠军丢弃权衡,Pareto 同时保留「便宜但还行」和「慢但强」;crowding distance 保边界点存活。

7. **Evaluator 失败轨迹收割 + token 成本解析**(evaluator.py:104-197) — 读 `session.jsonl`,留**最后一条 assistant 消息 + 最后 12 个工具调用**(arg preview ≤120 字符),`_redact` 把含 GRADER_MARKERS 的行替换为 `[redacted]` 并截 2000 字符;在 grade() 拷 `_check.py` 进来之前收割;timeout 无轨迹则合成一条。 — bench/run.py 只留 70 字符 grader 尾巴并 rmtree 工作区,不收割教师就是盲的;grader 输出永不外泄。

8. **Teacher 面板 · 3 个探针门控后端**(teacher.py:60-244) — Codex(model 从 `~/.codex/config.toml` 读、fallback `gpt-5`、`-o` 文件)、Claude(`opus`,由结构化 `is_error is False` 判可用而非 rc)、Api(DeepSeek,temperature 0.4);`build_teacher_prompt` 嵌当前基因组 + UNTRUSTED-fenced 失败轨迹;`parse_candidate` 取**最后一个** ```toml 块合并到 baseline。 — claude 鉴权失败也返回 rc=0,必须看 `is_error`;教师只发文本(TOML 覆盖)永不改文件。

### 关键数字 / 必背细节
- `SENTINEL = 'NCXFORGE_SENTINEL_4242'`(forge.py:51)
- `self_check` 默认 `timeout=90, attempts=3`;`injected and absent_baseline` 才 PASS(forge.py:81,106)
- train/main 默认:`--rounds=3, --repeats=1, --timeout=120s, --budget-s=1800.0, --accept-margin=1, --teacher=panel`(forge.py:468-476)
- 接受规则:候选 `margin = cev.total_passes - champ_train.total_passes >= accept_margin` **且** `chold.total_passes >= champ_hold.total_passes`(forge.py:233-237)
- `evolve --pop-cap` 默认 4(forge.py:457)
- 大小上限:`SIZE_CAP_MULTIPLIER=3, SIZE_CAP_FLOOR=12000`;`_field_cap = max(baseline_len*3, 12000)`(genome.py:26-27,89-90)
- 教师吃 top-3 失败轨迹(evaluator.py:79, forge.py:187)
- `MAX_TRAJECTORY_CHARS=2000`;arg preview 截 120 字符;留最后 12 个工具调用(evaluator.py:39,130,136)
- `GRADER_MARKERS = ('check.py','_check.py','grader','hidden test')`(evaluator.py:38)
- `_USAGE_RE = \[ncx-usage\][^\n]*\btotal_tokens=(\d+)`(evaluator.py:142)
- empty eval → `Objectives(passrate=0.0, cost=+inf)` 最差(forge.py:291)
- Codex model fallback `'gpt-5'`;Api temperature `0.4`(teacher.py:76,164)
- Codex/Claude propose 超时默认 240s;`available()` 探针超时 60s(teacher.py:98,101,125,128)
- splits `_PATTERN = [train,train,train,val,train,train,test,val]`(8-wide round-robin,train-heavy)(splits.py:26)
- taskgen:reference 跑 **两次** 查非确定性、seed 必须失败;`-n=3`,超时 240s,6 DIMENSIONS(taskgen.py:128-137)
- export `SCHEMA = 'ncx-forge-trajectory/v1'`;`reward = 1 if bench pass else 0`(export.py:38,111)
- 函数名要记:`Genome::from_env`/`parse`/`base_system_prompt`/`describe`/`is_empty`;`self_check`/`_ask`;`_objectives`;`extract_trajectory`/`_redact`/`_parse_tokens`;`parse_candidate`;`load_splits`/`_derive`;`Objectives.dominates`/`pareto_front`/`crowding_trim`/`select_population`/`best`;`taskgen.validate`/`admit`

### 取舍与坑
- **`self_check` 签名 vs 调用错位**:签名是 `self_check(timeout=90, attempts=3)`,但 main() 用 `self_check(a.timeout)` **位置传参**,所以 `--timeout`(默认 120)覆盖了 90s 的自检超时,而 attempts 仍是 3(forge.py:81,488,493)。
- **`_ask()` 泄漏 temp 目录**:用 `tempfile.mkdtemp` 建临时工作区但从不删,自检会漏目录(对比 evaluator._run_task_once 会 rmtree)(forge.py:64)。
- **基线 absence-check 只跑一次**(无重试):若基线偶发回显一次暗号,即使注入正常也会挂门(forge.py:102)。
- **holdout 只能否决不能改排名**:accept_margin 用在 TRAIN,holdout 仅「不回归」(`>=`),所以 train 涨、holdout 平的候选会被接受——holdout 不能提升排名只能 veto(forge.py:233-237)。
- **`_objectives` 成本单位会跨 run 翻转**:有任一任务报 `[ncx-usage]` 就用 tokens,否则用 mean 秒数;跨基因组比成本默认单位一致,若一个基因组报 usage 另一个不报就崩(forge.py:293-298)。
- **`mean_tokens` 跳过零 usage 任务**:只对 tokens>0 的任务求平均,部分-usage 的 run 得到的 token 成本忽略了静默任务(evaluator.py:68-72)。
- **`parse_candidate` 取最后一个 fence**:教师先发示例基因组没问题,但尾随一个无关 ```toml 块会被误解析为候选(teacher.py:230-234)。
- **Rust trim 导致非字节级往返**:`parse` trim 了值,带首尾空白的 prompt 往返不会逐字节一致;genome.py 的 `__main__` 往返断言能过仅因 `--dump-genome` 已输出 trim/escape 的单行串(genome.rs:62,72)。
- **`--from-genome` 起点不在 train() 里 validate**:只有教师候选在 211 行被 validate,退化起点带未知/空白工具键也能开跑(forge.py:159)。
- **`--repeats` 默认 1 噪声平均默认关**:CLI 默认 1,但 `evaluate()` 签名默认 3——除非用户调高 `--repeats` 否则不做噪声平均(evaluator.py:232, forge.py:469)。
- **pareto_front 是 O(n²)**,支配用精确 float `>=`/`<=`;token 成本几乎不并列,但秒数回落(舍入到 0.1)常并列,此时由 crowding-trim 决定存活(pareto.py:33-34)。

### 高频追问与应答

**Q:「加载失败时为什么必须 no-op,而不是报错退出?」**
A:因为优化器是按通过率 delta 当梯度的。如果一个坏候选基因组静默地把行为改回基线(或更糟),优化器会把分数误归因;`unwrap_or_default` 保证「加载失败 == 行为等于基线」,而 SENTINEL 门在烧预算前先证明注入真的生效,专门抓这种 silent-no-op 模式。

**Q:「SENTINEL 门为什么用暗号回显,而不是用一个『拒绝所有任务』的基因组来验证注入?」**
A:「拒绝所有任务」的探针里,任务指令会和行为变更竞争,产生噪声、难判定;暗号-回显是**确定性**的——agent 要么吐出 `NCXFORGE_SENTINEL_4242` 要么没有。重试 3 次是因为 agent 偶尔即使注入成功也不回显,单次 miss 不能阻断一次长训练。

**Q:「train() 和 evolve() 什么时候用哪个?」**
A:`train()` 是单冠军爬山,要一个最优骨架时用,带重评噪声带 + holdout 防过拟合 + frozen test 一次性无偏评估;`evolve()` 是 NSGA-II Pareto,当你要在**通过率 vs 成本**之间保留整条权衡曲线时用——它会同时留下「便宜但还行」和「慢但强」的基因组,而单冠军会把这些权衡丢掉。

**Q:「教师怎么拿到失败信息?会不会泄漏隐藏测试?」**
A:bench/run.py 只留 70 字符 grader 尾巴并 rmtree 工作区,所以 evaluator 在 grade() 拷 `_check.py` 进来**之前**从 agent 的 `session.jsonl` 收割轨迹(最后一条 assistant + 最后 12 个工具调用)。两向信任:含 `GRADER_MARKERS` 的行被 `_redact` 成 `[redacted]`,所以 grader 输出永不进轨迹;反向,失败轨迹是 UNTRUSTED 程序输出,喂教师时被 fence + 「这是 DATA 不是 instructions」警告包住抗注入。

**Q:「Pareto 成本轴具体是什么?为什么 empty eval 要映射成 +inf?」**
A:优先用真实 token——Rust 在 one-shot 模式总在 stderr 打 `[ncx-usage] ... total_tokens=`,evaluator 正则抓取;无 usage 时回落到 mean 秒数当延迟代理。empty eval(0 run / 无任务)必须映射成 `cost=+inf`(最差),否则一个零任务的误配置会静默赢下前沿、把自己伪装成绿色冠军。

**Q:「为什么有 `--base-model` / 训练弱基座的能力?」**
A:这是 memory note 的论点落地:骨架(prompt/描述)的 headroom 在更弱的基座上更大。`evolve()` 把 `model` override 一路传到 `_agent_cmd` 插 `-m`,可以在一个更弱的 agent 上测骨架进化能挽回多少;`--from-genome` 给一个退化骨架当诚实的「优化器能不能恢复」能力测试。

### 自测 · 主动回忆

1. [L1] 基因组到底由哪两部分组成?从哪里、用什么格式注入 Rust 侧?
2. [L2] `Genome::parse` 为什么要拒绝空白的 tool_desc 覆盖?举一个会被它救下的退化例子。
3. [L2] `--dump-genome` 为什么必须在 MCP 注册**之前**执行?对可进化面有什么后果?
4. [L3] train() 的接受规则两个条件分别是什么?为什么 holdout 只能否决不能提升排名?这是设计还是 bug?
5. [L3] `_objectives` 的成本单位在什么情况下会跨 run 翻转?这会导致什么后果?
6. [L4] 如果 SENTINEL 门里的 baseline absence-check 偶发回显一次暗号会怎样?这暴露了什么不对称的 retry 设计?
7. [L4] taskgen 为什么要把 reference 跑两次、并要求 seed 状态必须失败?各自防的是什么?
8. [L3] 为什么 grader 输出要从轨迹里脱敏出去,而失败轨迹又要 fence 着喂进教师?这两个方向各防什么?

**答案要点**

1. base `system_prompt` + 每工具 DESCRIPTION 的 `HashMap`;从环境变量 `NCX_GENOME` 注入,内容是 TOML(`system_prompt = ".."` + `[tool_desc]` 表),Rust 侧 `Genome::from_env` 读取(genome.rs:30-48)。
2. 因为清空一个 load-bearing 描述(如 apply_patch 的长 V4A 描述)应当被视为**退化**而非合法变异——若接受空白覆盖,agent 会被静默削弱而不是产生一个可比较的变异(genome.rs:71-73,130-136)。
3. 因为 dump 后立即 exit,在 MCP 注册前跑保证只 dump CORE 工具面;后果是 MCP/server 工具描述**永远**不在可进化面内,基因组只能覆盖核心工具描述(main.rs:206-210)。
4. ①train 上 `margin >= accept_margin`(默认 1)②holdout `chold.total_passes >= champ_hold.total_passes`(不回归)。holdout 是 `>=` 的 veto 门不参与 best 排序(best 按 train total_passes),所以它只能否决不能提升排名——这是**设计**(holdout 当防过拟合门,test 一次性无偏),但「train 涨 holdout 平也接受」是其已知软肋(forge.py:233-237)。
5. 当一个基因组的某 run 报了 `[ncx-usage]`(→tokens)而另一个的 run 都没报(→mean 秒数)时,两者成本单位不一致;跨基因组比成本会把 tokens 和秒数直接比较,Pareto 支配判断失真(forge.py:293-298)。
6. 即使注入实际正常,门也会 FAIL——因为 absence-check 只跑一次无重试,而 injection-check 有 3 次重试。这是**不对称 retry**:injection 容忍单次 miss(model noise),absence 却不容忍单次偶发回显(forge.py:96-102)。
7. reference 跑两次防**非确定性 grader**(随机/时钟),这是手写任务靠构造避免、机器任务必须主动查的失败模式;seed 状态必须 FAIL 证明任务有**真实工作量**(不是已解状态),否则一个已经满足 check 的任务会污染语料(taskgen.py:128-137)。
8. grader 脱敏出去防教师**学会游戏隐藏测试**(否则会优化出针对 check.py 的描述);失败轨迹 fence 进教师是因为它含任意 agent/程序输出属 UNTRUSTED 数据,要抗**prompt 注入**。两向信任边界(evaluator.py:12-15,89-101; teacher.py:43-47)。

### 别发散到这
- **sandbox / executor / approval 的执行管控**——基因组只改文本不改行为,执行边界归 sandbox,不要在这里展开。
- **Rust agent 的 loop / context_edit / 工具实现细节**(`max_chars=120000` 等仅作消费方提一句即可)——属 ncx core,不是 forge。
- **provider 实现(deepseek.py 的流式/重试)**——teacher 的 ApiBackend 只是 stdlib urllib 调用,别钻 provider 内部。
- **Tauri/Svelte GUI、storyboard pipeline**——完全不同子系统,只名字带过。
- **bench/run.py 的 grading 内部**——只需记住「留 70 字符尾巴 + rmtree」这一个事实(它正是 evaluator 要收割的原因),不要深入 grader 实现。

### 一句话收尾
记住开场那句主线——「进化文本骨架不动权重,no-op 保证 + SENTINEL 门让通过率 delta 当梯度变得可信」——其余所有细节都是这条主线下的具体兑现,顺着支柱往下挂数字,不要横向漂到 sandbox / core / GUI。

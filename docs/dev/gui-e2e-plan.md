# GUI 端到端测试：计划与实施指导

本文是交给实施者（AI 或人）的工作说明。实施完成后会有一次 review，第 9 节是 review 时的检查清单。
仓库根目录的 `AGENT.md` 优先于本文；两者冲突时以 `AGENT.md` 为准，并在 PR 描述里指出冲突。

## 1. 背景

- 现有 307 个测试中，**没有任何一个经过 `MainView`**。`crates/app/tests` 里的 7 个测试直接调用
  `RunGate` 和 `SyncEngine`；`--self-test-tray` 只检查窗口隐藏和恢复，不碰任务。
- 因此 `MainView` 的接线没有任何覆盖：编辑器保存 → `Workspace::upsert_task` → 落盘；卡片的运行按钮
  → `run_task` → `refuse_run` / `apply_statuses`；`review_deletions` → 确认窗口 → 只重跑被确认的目的地；
  触发器启动失败 → 卡片角标。
- 分支 `test/gui-e2e-spike` 上的试验提交已经验证可行（在它之上继续，不要另起分支）：
  - `crates/app/tests/gui_create_and_run.rs` 用 GPUI 的无窗口测试平台，从点击“新建任务”开始，
    选择文件夹、保存、添加目的地、运行，最后检查磁盘上的文件和 `status.json`。本地 0.4 秒内通过。
  - 变异验证：删掉 `MainView` 里的 `view.workspace.apply_statuses(statuses)`，测试按预期失败。
  - `cargo fmt --check`、`cargo clippy --workspace --all-targets --all-features`（`-D warnings`）、
    `cargo test --workspace`、`cargo check --release` 全部通过。
  - **还没在 GitHub Actions 上跑过。** 第一个 PR 推送后要确认 CI 通过（见第 7 节）。

**目标：** 补上 GUI → 状态 → 引擎 → 文件系统这条链路的覆盖。**不以减少单元测试数量为目标**；只有在第 8 节
规定的条件下才删除单元测试。

## 2. 已经搭好的基础（不要重做）

| 内容 | 位置 | 说明 |
|---|---|---|
| 测试平台依赖 | `crates/app/Cargo.toml` `[dev-dependencies]` | `gpui = { package = "gpui-pre", features = ["test-support"] }`。**不要**改成 `gpui-kit` 的 `test-support`：它把 `gpui-component` 列为弱依赖，Cargo 会把它解析进 lock，而 crates.io 上没有那个版本，解析直接失败。 |
| 按钮选择器 | `src/ui/button.rs` | 每个 `Button` 自动以它的 id 作为 `debug_selector`。`Button::new("task-save")` 在测试里就叫 `"task-save"`。 |
| 输入框选择器 | `src/ui/input.rs` | 输入框没有自己的 id，需要在调用处显式写 `Input::new(&state).debug_selector("task-name")`。 |
| 测试宏 | `#[gpui::test]` | dev-dependency 名字就叫 `gpui`，所以宏可以直接用。 |

`debug_selector` 在非测试构建里是空操作（GPUI 用 `cfg(any(test, all(debug_assertions, feature = "test-support")))`
控制），不影响 release 构建。

## 3. 第一步：抽出测试脚手架

新建 `crates/app/tests/gui_support/mod.rs`，每个 `gui_*.rs` 用 `mod gui_support;` 引入（与
`crates/core/tests/common/mod.rs` 的做法一致；不是每个文件都会用到每个 helper，按那边的方式处理
dead-code 警告）。把 spike 里的 `settle` / `click` / `pick_folder` 移过去，再补齐下面这些：

```rust
pub struct Harness { /* TempDir 们、Arc<dyn FileSystem>、ConfigLocation */ }

impl Harness {
    pub fn physical() -> Self;                 // 临时目录 + PhysicalFileSystem
    pub fn in_memory(fs: InMemoryFileSystem) -> Self; // 故障注入用，配置目录也在内存里
    pub fn seed_tasks(&self, tasks: &[SyncTask]);     // 启动前写入“手改过的配置”
    pub fn tasks(&self) -> Vec<SyncTask>;             // 通过 TaskStore 读盘
    pub fn statuses(&self) -> HashMap<Uuid, DestinationSyncStatus>;
    pub fn settings(&self) -> AppSettings;
    pub fn launch<'a>(&self, cx: &'a mut TestAppContext) -> &'a mut VisualTestContext;
}

pub fn settle(cx: &mut VisualTestContext);
pub fn click(cx: &mut VisualTestContext, selector: &str);   // 找不到或尺寸为 0 时 panic 并报出选择器名
pub fn is_shown(cx: &mut VisualTestContext, selector: &str) -> bool;
pub fn type_into(cx: &mut VisualTestContext, selector: &str, text: &str);
pub fn pick_folder(cx: &mut VisualTestContext, folder: &Path);
pub fn cancel_folder_picker(cx: &mut VisualTestContext);
pub fn other_window(cx: &mut VisualTestContext, main: AnyWindowHandle) -> VisualTestContext;
```

要点：

- **`launch` 必须照 `main.rs` 的顺序搭建**：`gpui_kit::init` → `syncmaid::ui::init` → `syncmaid::theme::install`
  → `Workspace::load` → `Root::new(MainView::new(..))`。**不要**调用 `platform::tray::start`、
  `close_to_tray`、`keep_hidden_across_display_changes`、`single_instance::acquire`：它们需要真实的
  Win32 窗口句柄或系统对象，测试平台没有。
- **点击前必须 `settle`。** 侧滑面板有 220 ms 的入场动画。动画中途点击时，按下落在这一帧，抬起落在下一帧，
  而那时按钮已经移动，两者不在同一个元素上就不算点击。测试平台的时钟是模拟的，
  `cx.executor().advance_clock(..)` 几乎没有开销。
- `debug_bounds` 只接受 `&'static str`。带 uuid 的 id 是运行时生成的，spike 用 `Box::leak` 处理，测试里可以接受。
- **辅助窗口**（批量删除确认、日志窗口）是独立的顶层窗口：用 `cx.windows()` 找到新窗口，再用
  `VisualTestContext::from_window(handle, &cx.cx)` 在那个窗口里点击。
- `VisualTestContext` 可以解引用为 `TestAppContext`，所以 `did_prompt_for_paths`、
  `simulate_path_prompt_response`、`windows()` 都能直接调用。

## 4. 第二步：给组件补选择器

**改一处，所有调用点自动获得**（与 `Button` 相同的写法，在组件内部用 id 生成选择器）：

| 组件 | 文件 | 用途 |
|---|---|---|
| `ChoiceCard` | `src/components/choice_card.rs` | `kind-sync` / `kind-move` / `strategy-mirror` / `strategy-add-only` |
| `Checkbox`、`Switch` | `src/ui/toggle.rs` | `verify-contents`、`confirm-large`、`settings-close-to-tray` 等 |
| `Link` | `src/ui/link.rs` | `status-link-{destination_id}`（打开日志窗口） |
| `Alert` | `src/ui/alert.rs` | 用来断言“提示出现了”，例如 `source-overlap`、`destination-blocked`、`workspace-blocked`、`config-unreadable`、`rule-error-{n}` |

**需要在调用处补的输入框选择器**（只补测试用到的，命名用 kebab-case，与按钮 id 风格一致）：

| 输入框 | 文件 | 建议选择器 |
|---|---|---|
| 目的地名称 / 路径 | `destination_editor.rs` | `destination-name` / `destination-path` |
| 规则组输入框 | `destination_editor.rs`，`group_inputs` 渲染处 | `rule-input-{index}`（`Input::debug_selector` 接受运行时字符串） |
| 批量删除阈值 | `destination_editor.rs` | `mass-delete-threshold` |
| cron / 稳定期 | `task_editor.rs` | `task-cron` / `task-settle` |

**选择器在一帧内必须唯一。** `debug_bounds` 背后是一张 map，重复的名字会悄悄只留下一个。已知的重复：
每张卡片的 `trigger-error`（`main_view.rs` 的 `trigger_error_badge`）；task editor 里不同状态共用的 Alert id `cron`；
destination editor 里每个规则组都有的 `path` / `extension` / `wildcard` / `regex` / `any` / `all` 按钮
（只有一个规则组时不冲突）。
测试需要用到时，把 id 改成按任务区分（例如 `trigger-error-{task_id}`）；改 element id 不改变行为。

## 5. 约束与禁区

**绝对不要在测试里点击的控件：**

- `settings-startup`：写 `HKCU\...\Run` 注册表，会改动开发者机器和 CI runner 的真实自启动项。
- `open-data-folder`、`log-open-file`：调用 `ShellExecuteW` 打开资源管理器或编辑器。
- 在 `PhysicalFileSystem` 上运行**默认删除模式**的 Mirror：删除的文件会进入真实回收站。会产生删除的场景
  一律用 `InMemoryFileSystem`；如果必须用真实文件系统，先在编辑器里点 `delete-permanent`。

**全局状态：** 每个 `tests/*.rs` 是一个独立进程，但同一文件里的测试会并行运行在不同线程上。

- `i18n::set_language` 是进程级全局状态 → 语言相关测试单独放进 `gui_settings.rs`，该文件内不要有其他依赖语言的测试。
- `services::logging::install` 安装全局 tracing subscriber → 日志窗口测试单独放进 `gui_log_viewer.rs`。

**测试平台做不到的事（不要尝试，保留现有测试）：**

- **读取界面上渲染出的文字。** 断言只能落在：磁盘文件（`tasks.json` / `status.json` / `settings.json`
  和目标目录）、选择器是否出现、窗口数量、文件夹选择框是否弹出。卡片文案由现有纯函数单元测试负责
  （`status_text`、`filter_summary`、`health_of`、`mirror_delete` 文案测试），**保留它们**。
  `status.json` 里的 `error` 字段是数据，可以和 `syncmaid::strings::*` 生成的句子比较。
- **运行中途点“停止”。** 引擎在后台任务里一次轮询就同步执行完，没有可以插入点击的时间点。取消语义继续由
  `core/tests/engine.rs` 的 `cancelling_mid_run_*` 和 `state/run_gate.rs` 覆盖。
- **Watch / Scheduled 触发器真正触发。** 它们跑在真实的系统线程和真实时钟上，与确定性调度器配合不稳定。
  继续由 `tests/self_triggering_run.rs` 和 core 的 triggers 测试覆盖。E2E 里一律使用 Manual 触发器。
  （例外：cron 无效导致的**启动失败**是同步发生的，可以测，见场景 T1。）
- 托盘、窗口隐藏/显示、单实例、自启动注册表：属于平台层，继续由平台测试和 `--self-test-tray` 负责。

## 6. 场景清单

每个场景写明：前置条件 → 操作（选择器）→ 断言 → **用来证明测试有效的变异**。实施时必须真的做一次变异，
看到测试失败后再恢复（见第 8 节），并在提交说明里写明用的是哪个变异。

### 6.1 `gui_create_and_run.rs`（已有一条，再扩展）

- **C1 已有：** 新建任务 → 添加目的地 → 运行 → 文件和状态落盘。
- **C2 编辑已有任务，保留 id 和目的地。** 预置一个带目的地的任务 → `edit-{task_id}` → 改名 → `task-save`
  → `tasks.json` 中 id 不变、目的地列表不变、名字已更新。
  变异：`TaskEditor::save` 里去掉 `task.id = self.task_id`。
- **C3 任务刚创建时默认展开。** C1 保存目的地后，`edit-dest-{destination_id}` 可见；点 `expand-{task_id}`
  后不可见。变异：`Workspace` 的默认展开逻辑。
- **C4 Run all。** 预置两个互不重叠的任务 → `run-all` → 两个任务的目标目录都有文件。

### 6.2 `gui_editor_guards.rs`：编辑器拦截（任务形状规则）

磁盘断言统一用“文件字节不变”或“文件不存在”，不要只断言“没报错”。

- **E1 名字为空不能保存。** `new-task` → 只选源文件夹 → `task-save` → `tasks.json` 不存在。
- **E2 源路径与其他任务重叠。** 预置任务 A（源 `X`）→ 新建任务，源选 `X\sub` → `source-overlap` 出现 →
  `task-save` → `tasks.json` 字节不变。变异：`TaskEditor::can_save` 去掉 `source_conflict` 检查。
- **E3 编辑任务时不与自己冲突。** 预置任务 A → `edit-{A}` → 不改源路径，直接 `task-save` → 成功写盘。
  变异：`source_conflict(&self.tasks, Some(self.task_id), ..)` 的 `Some(self.task_id)` 改成 `None`。
- **E4 目的地嵌套在源里（两个方向）。** 目的地选 `源\out`，以及选源的父目录 → `destination-blocked` 或
  `workspace-blocked` 出现 → `workspace-save` → `tasks.json` 字节不变。
- **E5 目的地与其他任务的目的地重叠。** 同 E4 的断言方式。
- **E6 Move 规则没有选择任何文件时不能保存。** `kind-move` 新建任务 → `add-{id}`（Move 规则默认为空选择）→
  `workspace-save` 被拦截 → 在 `rule-input-0` 输入 `pdf`，选扩展名类型 → `add-rule-0` → `workspace-save` 成功，
  `tasks.json` 里该目的地为 Move、规则是 pdf 扩展名。
- **E7 无效正则不进入规则。** 在规则输入框输入 `(`、选 `regex` → `add-rule-0` → `rule-error-0` 出现，规则列表不变。
  另外预置一个手改配置，在嵌套组里放无效正则 → 打开工作区 → `workspace-save` 被拦截，`tasks.json` 字节不变。
- **E8 Mirror 只保存全部文件过滤器。** 新目的地 → `strategy-mirror` → 保存 → `tasks.json` 中该目的地 filters
  正好是一个 AllFiles。变异：`build_destination` 里 Mirror 分支改为 `self.filters.to_rules()`。
- **E9 类型锁定。** 预置有目的地的任务 → `edit-{id}` → 点 `kind-move` → 保存后 kind 仍是 Sync。

### 6.3 `gui_run_outcomes.rs`：运行结果（以 `InMemoryFileSystem` 为主）

用 `Harness::in_memory`。路径用 Windows 形式的绝对路径（参考 `core/tests/common/mod.rs` 的 `C:\src`、`D:\dst`）。
任务可以预置（`seed_tasks`），因为编辑器已经在 6.2 测过；**运行必须从按钮触发**。

- **R1 批量删除：确认后只删这一次。** 源 2 个文件，Mirror 目的地另有 20 个孤儿文件 → `run-{task}` →
  `status.json` 为 `NeedsConfirmation`，孤儿文件一个不少 → `review-{destination}` → 新窗口出现 →
  在新窗口里点 `mirror-delete-confirm` → 孤儿文件被删，状态 `Success` → `tasks.json` / `status.json` 中
  没有任何“已批准”的痕迹 → 再放入 20 个孤儿文件 → `run-{task}` → 又回到 `NeedsConfirmation`。
  变异：`review_deletions` 里传入的 `HashSet::from([destination_id])` 改成空集合；或让批准写进配置。
- **R2 批量删除：保留 / 关窗都等于不删。** 同 R1，但点 `mirror-delete-keep`；再来一次，改为对确认窗口调用
  `simulate_close()`。两种情况孤儿文件都在，且没有发生第二次运行（状态仍是 `NeedsConfirmation`）。
- **R3 跨任务重叠在运行时被拒绝。** 预置两个共享目的地的任务（编辑器不允许，只能手改配置）→ `run-{A}` →
  A 的每个目的地都是 `Failed`，`error` 等于 `strings::task_overlap_refused_format(B 的名字)`，目标目录一个文件都没写。
  再测源路径重叠的情况。
- **R3b 链式任务允许运行。** 任务 A 的目的地是任务 B 的源 → 两个都能运行成功。
- **R3c 同一任务内两个目的地重叠交给引擎处理。** `error` 是引擎的英文句子，**不是**上面的拒绝句。
- **R4 文件被占用时延后。** `fs.lock_path(..)` 锁住一个源文件 → 运行 → `Incomplete`、`files_deferred == 1`，其余文件已复制。
- **R5 写入失败时报出路径和操作。** `with_faults(|f| f.fail_write_path_fragment = Some("b.txt".into()))` →
  `Failed`，`error` 里包含 `b.txt`，其他文件照常复制。
- **R6 空源目录的 Mirror。** 源为空、目的地有文件 → 运行 → 一个文件都不删，并且 `review-{destination}` **不出现**（不可覆盖）。
- **R7 Move 路由。** 预置 Move 任务，规则依次为 `*.pdf`、`invoices/`，源里放 `invoices/a.pdf`、`invoices/b.txt`、`c.pdf`、`d.txt`
  → 运行 → 每个文件只去第一条匹配的规则，`d.txt` 留在源里。
- **R8 删除任务和目的地。** `delete-{task}` → 确认对话框 `confirm-ok` → `tasks.json` 里没有该任务；
  `delete-dest-{id}` → `confirm-ok` → 目的地被移除，`status.json` 里它的状态也被清理。

### 6.4 `gui_persistence.rs`

- **P1 配置无法读取时不写盘。** 启动前往 `tasks.json` 写入非法内容 → 启动 → `config-unreadable` 出现 →
  通过 GUI 新建任务 → `tasks.json` 字节与写入时完全相同。变异：`Workspace` 里关掉“不可读时停止保存”的开关。
- **P2 重启后数据还在。** 同一个测试里，C1 流程结束后丢弃窗口，在同一数据目录上重新 `Workspace::load` 并开新窗口
  → `run-{task}` 仍然存在（选择器可见），运行成功。
- **P3 Move 规则顺序持久化。** 三条规则 → `up-{id}` / `down-{id}` 调整 → `workspace-save` → `tasks.json` 顺序与界面一致
  → 重启后顺序不变。变异：`save_destinations` 改为按旧顺序合并。

### 6.5 `gui_settings.rs`（独立文件，涉及全局语言）

- **S1** `sidebar-settings` → `settings-close-to-tray` → `settings.json` 更新，`MainView::close_to_tray()` 返回新值。
- **S2** 点 `language-{index}` → `settings.json` 的 language 为对应标签；选“跟随系统”时为空值。
- **不要碰 `settings-startup`。**

### 6.6 `gui_triggers.rs`

- **T1 触发器启动失败显示角标，修正后消失。** 预置一个 cron 无效的 Scheduled 任务 → 启动 → `trigger-error-{task}` 出现
  → `edit-{task}` → `trigger-manual` → `task-save` → 角标消失。需要先按第 4 节把该 id 改成按任务区分。

### 6.7 `gui_log_viewer.rs`（独立文件，涉及全局日志）

- **L1** 运行一个任务 → 点 `status-link-{destination}` → 窗口数 +1，新窗口里有 `log-lines` → 再点另一个目的地的链接
  → 窗口数不变（复用同一个窗口）。日志内容过滤由 `services/logging.rs` 的单元测试负责，保留它们。

## 7. CI

- 不需要改 workflow：`cargo test --workspace` 会自动运行 `tests/gui_*.rs`。
- `Cargo.lock` 变了，第一次 CI 会错过缓存，编译时间会变长；之后恢复正常。
- 整个 GUI 测试集合预计在几秒内跑完。如果单个测试超过 1 秒，多半是哪里用了真实的 sleep 或真实线程，应该改掉。
- 第一个 PR 推送后，**确认 `windows-latest` 上的 CI 通过**，再继续后续 PR。
- `--self-test-tray` 进 CI 是另一件事，不在本计划范围内。

## 8. 删除单元测试的规则

只在所有场景都合入之后做，单独一个提交。**每删一个，都要满足下面全部条件：**

1. 有一个 E2E 场景覆盖同一个行为，并且对该单元测试所针对的 bug 做变异后，**这个 E2E 测试会失败**。在提交说明里逐条写出
   “删除的测试 → 对应场景 → 使用的变异”。
2. 它不是持久化格式测试（`AGENT.md`：持久化格式要钉住），不是故障注入、竞态或边界输入测试。
3. 拿不准就保留。

候选清单（约 17 个，都在 `crates/app/src`）。**这只是候选，不是必删清单**：

| 单元测试 | 对应场景 |
|---|---|
| `state/overlap.rs`：`a_run_is_refused_when_either_end_overlaps_another_task` | R3 |
| `state/overlap.rs`：`a_run_that_shares_nothing_is_not_refused` | C1 |
| `state/overlap.rs`：`a_run_of_a_chained_task_is_allowed` | R3b |
| `state/overlap.rs`：`two_destinations_of_one_task_are_left_to_the_engine` | R3c |
| `state/overlap.rs`：`a_task_does_not_conflict_with_itself_while_being_edited` | E3 |
| `state/overlap.rs`：`a_destination_being_edited_is_not_its_own_conflict` | 编辑已有目的地后保存（在 E5 里加一条） |
| `state/workspace.rs`：`a_first_run_loads_empty_and_is_free_to_save` | C1 |
| `state/workspace.rs`：`unreadable_config_switches_persistence_off_rather_than_overwriting_it` | P1 |
| `state/workspace.rs`：`every_task_starts_expanded_and_selecting_one_opens_it` | C3 |
| `state/workspace.rs`：`settings_survive_a_reload` | S1 + 重启 |
| `state/filters.rs`：`no_rules_at_all_says_so_and_blocks_the_save` | E6 |
| `state/filters.rs`：`a_regex_that_does_not_compile_blocks_the_save_wherever_it_sits` | E7（必须包含嵌套组的情况） |
| `state/triggers.rs`：`a_trigger_that_will_not_start_is_reported_rather_than_thrown` | T1 |
| `state/triggers.rs`：`changing_the_trigger_or_the_source_rebuilds_the_runner` | T1（只有角标确实因重建而消失时才算覆盖） |
| `views/settings.rs`：两个语言下拉框值往返测试 | S2 |
| `state/run_gate.rs`：`an_approval_given_mid_run_survives_into_the_run_that_performs_it` | 与 `tests/mass_delete_approval.rs` 的同名场景重复，删哪个要说明理由 |

明确**保留**：`health`、`routing`、`preview`、`logging`、`i18n`、`main_view` / `mirror_delete` / `confirm` 的文案测试
（E2E 读不到文字），`run_gate` 的其余测试（竞态），以及 core 里的所有测试。

## 9. 交付顺序与提交

每一步单独提交，提交前跑 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features`
（`RUSTFLAGS=-D warnings`）、`cargo test --workspace`。提交信息使用 conventional commits 格式，**不加任何
Co-Authored-By、Assisted-by 或其他署名尾注**。

1. `gui_support` 脚手架 + 把 C1 迁过去。
2. 第 4 节的组件选择器。
3. 6.1 → 6.7，每个文件一个提交。
4. 在 `AGENT.md` 的 “Tests” 一节加一小段 GUI 测试约定：选择器从 id 生成、点击前 `settle`、断言落在磁盘而不是文字、
   第 5 节的禁区。
5. 第 8 节的删除（如果有）。

## 10. Review 检查清单

- 每个新测试都至少有一个**会失败**的断言，并且提交说明里写了变异验证。
- 没有点击第 5 节的禁区控件；会删除文件的场景用的是内存文件系统或永久删除模式。
- 没有 `std::thread::sleep`、没有依赖真实时钟；只用 `settle` / `advance_clock`。
- 为了测试而加到生产代码里的只有选择器（以及第 4 节允许的 id 调整），没有为了测试改变行为或暴露内部状态。
  确实需要新的公开接口时，在 PR 里单独说明理由。
- 同一个行为没有在多个层级重复测试（`AGENT.md`：在能看到它的最高层测一次）。
- `AGENT.md` 里的产品规则没有被绕开：例如没有为了让测试通过而允许嵌套布局或持久化批准。

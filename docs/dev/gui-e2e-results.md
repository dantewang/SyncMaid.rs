# GUI E2E 实施记录

在 `test/gui-e2e-spike` 上延续试验。测试通过无窗口 GPUI 平台点击真实组件，
由 MainView 调用工作区、引擎和文件系统；没有为测试增加生产接口或修改产品行为。

## 场景对应

| 测试文件 | 测试函数数 | 计划场景 |
|---|---:|---|
| gui_create_and_run.rs | 3 | C1、C2、C3、C4；P2 与 C1 合并，关闭旧窗口后重载并复制新增文件 |
| gui_editor_guards.rs | 7 | E1–E9；E3 由 C2 的同源路径编辑覆盖；E6 与交互式 E7 合并；E5 包含已有目的地改名保存 |
| gui_run_outcomes.rs | 10 | R1–R8，含 R3b、R3c；R3 在同一测试中检查源与目的地两类冲突 |
| gui_persistence.rs | 2 | P1、P3；P3 同时操作 down/up，重载后继续调整顺序 |
| gui_settings.rs | 1 | S1、S2；检查 MainView 的 close_to_tray，并关闭窗口重新加载 |
| gui_triggers.rs | 1 | T1 |
| gui_log_viewer.rs | 1 | L1；核对运行结果、日志选择器和复用窗口的句柄 |

合计 25 个 GUI 测试函数。部分连续行为合并在一个测试中，避免重复创建同一条用户旅程。
现有 307 个非 GUI Cargo 测试全部保留；合计 332 个 Cargo 测试函数。
托盘自测仍单独运行。

## 与原计划的细节差异

- R7 使用 `**/*.pdf`：项目通配符按路径段匹配，`*.pdf` 不覆盖 `invoices/a.pdf`。
  这保留了计划真正要验证的“重叠规则第一条优先”行为，没有改变过滤器实现。
- P3 使用三种不同扩展名规则，避免把 AllFiles 当成普通可移动规则；AllFiles 是必须排在最后的兜底规则。
- P2 放在 C1 末尾，复用由 GUI 创建并运行的任务。重启后的第二次运行必须复制新增文件。
- 关闭辅助窗口的按钮会同步销毁窗口。click 先记住窗口句柄，仅在点击后窗口仍存在时刷新它。
- Harness 保存创建时的 MainView Entity，仅用于读取已有公开 accessor；生产代码未增加测试接口。
- 只给测试实际使用的输入框增加选择器；没有给未用到的 cron、稳定期输入框增加选择器。
- 没有删除单元测试：尚未满足“所有场景合入后再逐个证明可删除”的条件。

## 变异验证

每次变异单独应用，运行对应测试，必须出现 `test result: FAILED`，然后在 finally 中恢复源文件。
编译错误不算杀死变异。测试中不保留变异开关。

| 变异 | 检测它的测试 |
|---|---|
| MainView 不回写 apply_statuses | a_task_made_in_the_window_runs_and_lands_on_disk |
| TaskEditor 保存时生成新 UUID | editing_preserves_identity_and_destinations |
| can_save 忽略 source_conflict | overlapping_source_is_blocked |
| source_conflict 不排除自身任务 | editing_preserves_identity_and_destinations |
| Mirror 构建目的地时保留原过滤器 | mirror_persists_only_all_files |
| 确认删除时传入空授权集合 | mirror_approval_is_one_shot |
| Workspace 忽略 config_unreadable | unreadable_config_is_preserved |
| save_destinations 反转规则顺序 | rule_order_survives_reload |
| Link 不调用激活回调 | status_links_open_and_reuse_log_window |
| can_save 忽略空名称 | unnamed_task_never_reaches_disk |
| 目的地检查忽略源与目的地嵌套 | nested_destinations_and_other_tasks_destinations_are_blocked |
| 目的地检查允许空规则 | move_requires_a_valid_rule_before_saving |
| 目的地检查跳过正则校验 | invalid_nested_regex_cannot_be_saved |
| 类型选择不再锁定 | existing_destination_can_be_saved_and_task_kind_stays_locked |
| 成功启动后不清除旧错误角标 | correcting_failed_trigger_removes_badge |
| 设置开关写回旧值 | settings_changes_persist |

额外断开通用 Button 点击回调，除日志窗口测试外的 24 个 GUI 测试全部失败。
日志窗口测试通过独立的 Link 回调变异验证。这个广泛的接线检查不替代上表中的规则变异。
第一次 Button 变异揭示日志测试仅打开窗口也能通过，因此补上了两个目的地已成功运行的前置断言。

另做过“已有触发器配置变化时不重建”的变异，T1 仍通过：无效 cron 创建失败，没有运行实例，修正时走的是首次成功创建。这说明 T1 不覆盖已有实例的重建，因此保留 `changing_the_trigger_or_the_source_rebuilds_the_runner`，不把角标消失当作重建覆盖证明。

## 本地检查与边界

使用 `RUSTFLAGS=-D warnings` 执行 fmt、workspace/all-targets/all-features Clippy、
`cargo test --workspace`，并检查 release 构建。结果：fmt、Clippy、332 个测试及 release 检查全部通过。

没有操作自启动、ShellExecute、真实回收站或托盘；Mirror 删除场景全部使用 InMemoryFileSystem。
没有增加 sleep；GUI 动画仅推进模拟时钟。真实触发、停止竞态和文案继续由已有测试覆盖。
没有修改 CI workflow，现有 `cargo test --workspace` 自动包含新增测试。
本次只在本地实施；GitHub Actions windows-latest 尚未验证，推送后仍需确认该结果。

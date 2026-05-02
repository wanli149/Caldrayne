main-username = 用户名
main-server = 服务器
main-password = 密码
main-connecting = 连接中
main-creating_world = 创建世界中
main-tip = 小提示:
main-unbound_key_tip = 自由
main-notice =
    欢迎来到卡德雷恩 Online（Caldrayne Online）Alpha 版本！

    在你开始游戏之前，请注意以下几点：

    - 这是一个仍在品牌化与开发中的早期版本，你会遇到 bug、未完成的玩法、尚未打磨的机制，以及暂未实装的内容。

    - 卡德雷恩 Online 源自 Veloren 开源基础。我们会保留必要的上游署名与许可证信息，同时逐步建立自己的品牌与内容方向。

    - 本项目依然遵循开源协作方式。贡献时请保留必要的上游许可证、署名与致谢信息。

    - 随着品牌化工作持续推进，当前部分内部模块、资源键与工程标识仍可能暂时保留历史遗留命名。

    感谢你抽出时间阅读这段说明，祝你游玩愉快。

    ~ 卡德雷恩开发团队
main-login_process =
    关于多人模式：

    某些服务器可能要求使用账号认证。

    如果服务器启用了身份验证，请使用服务器运营方提供的账号服务。
main-singleplayer-new = 新建
main-singleplayer-delete = 删除
main-singleplayer-regenerate = 重新生成
main-singleplayer-create_custom = 自定义
main-singleplayer-seed = 种子
main-singleplayer-day_length = 白天长度
main-singleplayer-random_seed = 随机种子
main-singleplayer-size_lg = 指数级大小
main-singleplayer-map_large_warning = 警告：大型世界首次启动时将花费很长时间。
main-singleplayer-world_name = 世界名称
main-singleplayer-map_scale = 地图缩放
main-singleplayer-map_erosion_quality = 地图侵蚀质量
main-singleplayer-map_shape = 地图形状
main-singleplayer-provenance = 历史来源：{ $source }
main-singleplayer-provenance-legacy_unknown = 旧版世界；原始来源不可用
main-singleplayer-provenance-legacy_migrated = 已迁移的旧版世界；原始来源不可用
main-singleplayer-provenance-load_path = 严格世界文件“{ $name }”
main-singleplayer-provenance-load_legacy_path = 旧兼容世界文件“{ $name }”
main-singleplayer-provenance-load_asset = 非默认世界资源“{ $asset }”
main-singleplayer-provenance-load_or_generate = 受管理世界“{ $name }”（{ $overwrite }）
main-singleplayer-provenance-overwrite-true = 允许覆盖
main-singleplayer-provenance-overwrite-false = 禁止覆盖
main-singleplayer-legacy_gap-missing_typed_origin = 此世界仍有遗留元数据缺口：缺少类型化来源记录
main-singleplayer-legacy_gap-missing_compat_audit = 此世界仍有遗留元数据缺口：缺少兼容审计
main-singleplayer-legacy_gap-missing_typed_origin_and_compat_audit = 此世界仍有遗留元数据缺口：缺少类型化来源记录；缺少兼容审计
main-singleplayer-legacy_gap-badge-missing_typed_origin = 缺来源
main-singleplayer-legacy_gap-badge-missing_compat_audit = 缺审计
main-singleplayer-legacy_gap-badge-missing_typed_origin_and_compat_audit = 缺来源+审计
main-singleplayer-managed_recipe_sidecar_missing = 此受管理世界缺少相邻 recipe sidecar；当前运行时 recipe 合同仍由 legacy 选项比较推断
main-singleplayer-managed_recipe_sidecar_missing-badge = 缺 sidecar
main-singleplayer-legacy_stock-total = 仍有遗留旧世界：{ $legacy }
main-singleplayer-legacy_stock-unknown = 其中来源未知 { $count }
main-singleplayer-legacy_stock-missing_typed_origin = 缺少类型化来源记录 { $count }
main-singleplayer-legacy_stock-missing_compat_audit = 缺少兼容审计 { $count }
main-singleplayer-legacy_stock-sidecarless_managed_residual = 仍属 sidecarless managed 残留 { $count }
main-singleplayer-play = 开始游戏
main-singleplayer-generate_and_play = 生成并开始游戏
menu-singleplayer-confirm_delete = 您确定要删除【{ $world_name }】吗？
menu-singleplayer-confirm_regenerate = 您确定要重新生成【{ $world_name }】吗？
main-login-server_not_found = 找不到服务器。
main-login-no_ip_addr = 未能解析出可用的服务器地址。
main-login-authentication_error = 服务器验证错误。
main-login-internal_error = 客户端发生内部错误。提示：当前角色数据可能已丢失或被删除。
main-login-failed_auth_server_url_invalid = 无法连接到身份验证服务器。
main-login-insecure_auth_scheme = 当前不支持 HTTP 身份验证，因为它并不安全。仅在 localhost 或调试版本中允许使用 HTTP。
main-login-server_full = 服务器已满。
main-login-untrusted_auth_server = 当前认证服务器未被信任。
main-login-timeout = 连接超时：服务器未能及时响应。可能是服务器负载过高，或当前网络状况不稳定。
main-login-server_shut_down = 服务器已关闭。
main-login-network_error = 网络错误。
main-login-network_wrong_version = 客户端与服务器版本不一致，请先确认游戏是否需要更新。
main-login-failed_sending_request = 认证服务器请求失败。
main-login-invalid_character = 所选角色无效。
main-login-bad_world_map_dimensions = 服务器发送的世界地图尺寸数据无效。
main-login-bad_world_map_image = 服务器发送的世界地图图像数据无效。
main-login-bad_altitude_map = 服务器发送的高度图数据无效。
main-login-entity_sync_failed = 无法从服务器同步玩家实体数据。
main-login-client_crashed = 客户端崩溃。
main-login-render_backend_failed = 无法选择可用的渲染后端。当前没有检测到兼容的图形后端。客户端目前支持 Vulkan、Metal、DX12 和 OpenGL。{ $potential_fix } 如果问题仍然存在，请在反馈时附上你的操作系统和显卡信息。
main-login-render_backend_failed-fix-windows = 更新当前系统的显卡驱动，通常就能解决这个问题。
main-login-render_backend_failed-fix-none = 当前没有可提供的额外修复建议。
main-login-render_backend_failed-fix-vulkan = 安装或更新 Vulkan 驱动，通常就能解决这个问题。
main-login-window_create_failed = 创建游戏窗口失败：{ $raw_error }
main-login-not_on_whitelist = 你尝试加入的服务器未将你列入白名单。
main-login-banned = 您已被永久封禁，理由如下：{ $reason }
main-login-kicked = 你已被踢出，理由如下：{ $reason }
main-login-select_language = 选择语言
main-login-client_version = 客户端版本
main-login-server_version = 服务端版本
main-login-client_init_failed = 客户端初始化失败：{ $init_fail_reason }
main-login-username_bad_characters = 用户名包含无效字符。仅支持字母、数字、`_` 和 `-`。
main-login-username_too_long = 用户名太长，最大长度为：{ $max_len }
main-servers-select_server = 选择服务器
main-servers-singleplayer_error = 无法连接到内部服务器：{ $sp_error }
main-servers-network_error = 服务器网络错误：{ $raw_error }
main-servers-participant_error = 连接对象断开或协议异常：{ $raw_error }
main-servers-stream_error = 客户端连接、压缩或数据解析异常：{ $raw_error }
main-servers-database_error = 服务器数据库错误：{ $raw_error }
main-servers-persistence_error = 服务器持久化错误（可能与资源文件或角色数据有关）：{ $raw_error }
main-servers-other_error = 服务器内部错误：{ $raw_error }
main-servers-world_compat_error = 兼容性强校验阻止了世界载入：{ $entry }因{ $failure }而无法使用。{ $remediation }
main-servers-world_compat_notice = 本次启动触发了世界兼容性回退：{ $entry }因{ $failure }未能载入，所以当前会话改为使用重新生成的世界继续启动。{ $remediation }
main-servers-world_compat_notice-load_legacy = 本次会话仍在使用过渡期世界兼容导入路径：{ $entry }通过显式兼容导入完成载入，而不是严格世界文件合同。{ $remediation }
main-servers-world_compat_entry-load = 当前选中的世界文件
main-servers-world_compat_entry-load_legacy = 旧版世界文件
main-servers-world_compat_entry-load_asset = 内置默认世界资源
main-servers-world_compat_entry-generic = 当前请求的世界输入
main-servers-world_compat_failure-missing_input = 缺少输入数据
main-servers-world_compat_failure-parse_error = 世界数据无法解析
main-servers-world_compat_failure-invalid_world = 世界数据无效
main-servers-world_compat_failure-option_mismatch = 生成参数已不再匹配
main-servers-world_compat_failure-generic = 未知的兼容性问题
main-servers-world_compat_remediation-load = 请恢复该世界文件，或在世界选择器中重新生成这个世界后再启动。
main-servers-world_compat_remediation-load_asset = 请恢复默认世界资源并检查安装内容是否完整，然后再启动。
main-servers-world_compat_remediation-generic = 请改用其他世界来源，或先重新生成世界后再启动。
main-servers-world_compat_notice_remediation-load = 如果你需要之前的世界数据，请在下次启动前恢复原始世界文件，或从备份中找回它。
main-servers-world_compat_notice_remediation-load_legacy = 如果你希望这个世界在旧兼容导入路径退场后仍能启动，请在后续启动前把它迁移到严格的世界文件加 sidecar 合同。
main-servers-world_compat_notice_remediation-load_asset = 如果你原本期望直接载入内置默认世界，请检查安装内容是否完整。
main-servers-world_compat_notice_remediation-generic = 如果这次回退不符合预期，请重新检查所选世界来源。
main-server-rules = 本服务器设有必须遵守的规则。
main-server-rules-seen-before = 这些规则自你上次确认后已更新。
main-credits = 鸣谢
main-credits-created_by = 创建了
main-credits-music = 音乐
main-credits-sound = 音效
main-credits-fonts = 字体
main-credits-other_art = 其他艺术
main-credits-contributors = 贡献者
loading-tips =
    .a0 = 按下 '{ $gameinput-togglelantern }' 点亮你的提灯。
    .a1 = 按下 '{ $gameinput-controls }' 查看默认按键列表。
    .a2 = 输入 /say 或 /s，可以和附近的玩家聊天。
    .a3 = 输入 /region 或 /r，可以和周围数百格范围内的玩家聊天。
    .a4 = 管理员可使用 /build 指令进入建造模式。
    .a5 = 输入 /group 或 /g，可以和当前队伍成员聊天。
    .a6 = 输入 /tell 玩家名 消息，可向指定玩家发送私聊。
    .a7 = 多留意地上的食物、箱子和其他战利品。
    .a8 = 背包里食物太多？不妨试试把它们做成更好的料理。
    .a9 = 不知道接下来做什么？可以先去地图上标出的地牢看看。
    .a10 = 别忘了根据你的设备调整画面设置。按下 '{ $gameinput-settings }' 打开设置。
    .a11 = 和别人一起冒险会更有趣。按下 '{ $gameinput-social }' 查看当前在线玩家。
    .a12 = 按下 '{ $gameinput-dance }'，随时来一段舞步。
    .a13 = 按下 '{ $gameinput-glide }' 展开滑翔翼，飞向天空。
    .a14 = Caldrayne Online 仍处于前期开发测试阶段，我们每天都在持续打磨它。
    .a15 = 如果你想参与开发共建或提交反馈，欢迎前往我们的 GitHub 仓库。
    .a16 = 你可以在设置中选择是否在生命条上显示具体血量。
    .a17 = 坐在篝火旁并按下 '{ $gameinput-sit }'，可以缓慢恢复生命值。
    .a18 = 想继续你的旅程，却缺更大的背包或更好的护甲？按下 '{ $gameinput-crafting }' 打开制作菜单。
    .a19 = 按下 '{ $gameinput-roll }' 可以翻滚。翻滚既能快速位移，也能躲开敌人的攻击。
    .a20 = 想查某件物品能拿来做什么？在制作界面搜索“input:<物品名称>”，就能看到相关配方。
    .a21 = 看到喜欢的画面？按下 '{ $gameinput-screenshot }' 把它截下来。
main-singleplayer-map_large_extra_warning = 生成 { $count } 个使用默认选项的世界大致需要花费等量的资源。
main-login-banned_until =
    你已被暂时封停，原因如下：{ $reason }
    解封时间：{ $end_date }
main-singleplayer-map_shape-circle = 圆形
main-singleplayer-map_shape-square = 方形

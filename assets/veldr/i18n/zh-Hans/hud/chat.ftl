hud-chat-all = 全部
hud-chat-chat_tab_hover_tooltip = 右键点击可打开聊天设置
hud-chat-online_msg = { "[" }{ $name }] 上线了。
hud-chat-offline_msg = { "[" }{ $name }] 下线了。
hud-chat-default_death_msg = { "[" }{ $name }] 已阵亡
hud-chat-fall_kill_msg = { "[" }{ $name }] 因坠落而亡
hud-chat-suicide_msg = { "[" }{ $name }] 死于自伤
hud-chat-died_of_pvp_buff_msg =
    .burning = { "[" }{ $victim }] 被 [{ $attacker }] 的燃烧效果烧死了
    .bleeding = { "[" }{ $victim }] 因 [{ $attacker }] 造成的流血而亡
    .curse = { "[" }{ $victim }] 死于 [{ $attacker }] 施加的诅咒
    .crippled = { "[" }{ $victim }] 因 [{ $attacker }] 施加的致残效果而亡
    .frozen = { "[" }{ $victim }] 因 [{ $attacker }] 施加的冰冻而亡
    .mysterious = { "[" }{ $victim }] 死于 [{ $attacker }] 施加的神秘力量
hud-chat-pvp_melee_kill_msg = { "[" }{ $attacker }] 击败了 [{ $victim }]
hud-chat-pvp_ranged_kill_msg = { "[" }{ $attacker }] 射杀了 [{ $victim }]
hud-chat-pvp_explosion_kill_msg = { "[" }{ $attacker }] 炸死了 [{ $victim }]
hud-chat-pvp_energy_kill_msg = { "[" }{ $attacker }] 用魔法击杀了 [{ $victim }]
hud-chat-pvp_other_kill_msg = { "[" }{ $attacker }] 杀死了 [{ $victim }]
hud-chat-died_of_buff_nonexistent_msg =
    .burning = { "[" }{ $victim }] 被燃烧致死
    .bleeding = { "[" }{ $victim }] 因流血而亡
    .curse = { "[" }{ $victim }] 死于诅咒
    .crippled = { "[" }{ $victim }] 因致残效果而亡
    .frozen = { "[" }{ $victim }] 因冰冻而亡
    .mysterious = { "[" }{ $victim }] 死于神秘力量
hud-chat-died_of_npc_buff_msg =
    .burning = { "[" }{ $victim }] 被 { $attacker } 的燃烧效果烧死了
    .bleeding = { "[" }{ $victim }] 因 { $attacker } 造成的流血而亡
    .curse = { "[" }{ $victim }] 死于 { $attacker } 施加的诅咒
    .crippled = { "[" }{ $victim }] 因 { $attacker } 施加的致残效果而亡
    .frozen = { "[" }{ $victim }] 因 { $attacker } 施加的冰冻而亡
    .mysterious = { "[" }{ $victim }] 死于 { $attacker } 施加的神秘力量
hud-chat-npc_melee_kill_msg = { $attacker } 击杀了 [{ $victim }]
hud-chat-npc_ranged_kill_msg = { $attacker } 射杀了 [{ $victim }]
hud-chat-npc_explosion_kill_msg = { $attacker } 炸死了 [{ $victim }]
hud-chat-npc_energy_kill_msg = { $attacker } 用魔法击杀了 [{ $victim }]
hud-chat-npc_other_kill_msg = { $attacker } 击杀了 [{ $victim }]
hud-loot-pickup-msg =
    { $amount ->
        [1] { $actor } 拾取了 { $item }
       *[other] { $actor } 拾取了 { $amount }x { $item }
    }
hud-chat-goodbye = 再见！
hud-chat-connection_lost = 连接中断。{ $time }秒后将被踢出。
hud-chat-group_message_hint = 输入 /g 或 /group 可以与队伍成员聊天。
hud-chat-group-joined = [{ $name }] 加入了队伍。
hud-chat-group-left = [{ $name }] 离开了队伍。
hud-chat-tell-to-npc = 你对 [{ $alias }] 说：{ $msg }
hud-chat-tell-from-npc = [{ $alias }] 对你说：{ $msg }
hud-chat-tell-to = 发给 [{ $alias }]：{ $msg }
hud-chat-tell-from = [{ $alias }] 对你说：{ $msg }
hud-chat-message = { "[" }{ $alias }]：{ $msg }
hud-chat-message-with-name = { "[" }{ $alias }] { $name }：{ $msg }
hud-chat-message-in-group = ({ $group }) [{ $alias }]：{ $msg }
hud-chat-message-in-group-with-name = ({ $group }) [{ $alias }] { $name }：{ $msg }
hud-loot-pickup-msg-you =
    { $amount ->
        [1] 你拾取了 { $item }
       *[other] 你拾取了 { $amount }x { $item }
    }
hud-chat-singleplayer-motd1 = 整个世界都只属于你，尽情大展身手吧……
hud-chat-singleplayer-motd2 = 这片宁静如何？

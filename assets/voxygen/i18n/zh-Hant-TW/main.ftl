main-username = 使用者名稱
main-server = 伺服器
main-password = 密碼
main-connecting = 連線中
main-creating_world = 建立世界
main-tip = 提示：
main-unbound_key_tip = 未綁定
main-notice =
    歡迎來到卡德雷恩 Online（Caldrayne Online）Alpha 版本！

    在你開始遊戲之前，請先留意以下幾點：

    - 這是一個仍在品牌化與開發中的早期版本，你可能會遇到 bug、未完成的玩法、尚未打磨的系統，以及暫未實裝的內容。

    - 卡德雷恩 Online 建構於 Veloren 開源基礎之上。我們會保留必要的上游署名與授權資訊，同時逐步建立自己的品牌與內容方向。

    - 本專案仍採用開源協作模式。貢獻時請保留必要的上游授權、署名與致謝資訊。

    - 基於相容性考量，目前部分內部模組、資源鍵與工程標識仍可能保留歷史 `veloren` 命名。

    感謝你花時間閱讀這段說明，祝你遊玩愉快。

    ~ 卡德雷恩開發團隊
main-login_process =
    關於多人模式：

    某些伺服器可能要求使用帳號驗證。

    如果伺服器啟用了身份驗證，請依照伺服器營運方提供的帳號服務說明操作。
main-singleplayer-new = 新遊戲
main-singleplayer-delete = 刪除
main-singleplayer-regenerate = 重新生成
main-singleplayer-create_custom = 創建自訂遊戲
main-singleplayer-seed = 種子
main-singleplayer-day_length = 一天的長度
main-singleplayer-random_seed = 隨機
main-singleplayer-size_lg = 指數大小
main-singleplayer-map_large_warning = 警告：大地圖第一次啟動會花費較長時間
main-singleplayer-world_name = 世界名稱
main-singleplayer-map_scale = 垂直比例
main-singleplayer-map_erosion_quality = 侵蝕品質
main-singleplayer-map_shape = 地圖形狀
main-singleplayer-map_shape-circle = 圓形
main-singleplayer-map_shape-square = 正方形
main-singleplayer-play = 開始遊戲
main-singleplayer-generate_and_play = 生成並遊玩
menu-singleplayer-confirm_delete = 您確定要刪除 "{ $world_name }" 嗎？
menu-singleplayer-confirm_regenerate = 您確定要重新生成 "{ $world_name }" 嗎？
main-login-server_not_found = 找不到伺服器
main-login-authentication_error = 伺服器認證錯誤
main-login-internal_error = 客戶端內部錯誤，提示：可能角色已被刪除
main-login-failed_auth_server_url_invalid = 無法連接到認證伺服器
main-login-insecure_auth_scheme = HTTP 認證協議不受支持，這不安全！出於開發目的，'localhost' 或除錯構建允許 HTTP
main-login-server_full = 伺服器已滿
main-login-untrusted_auth_server = 認證伺服器不被信任
main-login-timeout = 超時：伺服器未及時回應，提示：伺服器可能目前過載或網路有問題
main-login-server_shut_down = 伺服器已關閉
main-login-network_error = 網路錯誤
main-login-network_wrong_version = 客戶端和伺服器版本不匹配，提示：您可能需要更新遊戲客戶端
main-login-failed_sending_request = 認證伺服器請求失敗
main-login-invalid_character = 選定的角色無效
main-login-client_crashed = 客戶端崩潰
main-login-not_on_whitelist = 您不在嘗試連接的伺服器的白名單中
main-login-banned = 您已被封禁，原因如下：
main-login-kicked = 您已被踢出，原因如下：
main-login-select_language = 選擇語言
main-login-client_version = 客戶端版本
main-login-server_version = 伺服器版本
main-login-client_init_failed = 客戶端初始化失敗：{ $init_fail_reason }
main-login-username_bad_characters = 使用者名稱包含無效字元！（僅允許字母、數字、'_' 和 '-'）
main-login-username_too_long = 使用者名稱過長！最大長度為：{ $max_len }
main-servers-select_server = 選擇伺服器
main-servers-singleplayer_error = 連接內部伺服器失敗：{ $sp_error }
main-servers-network_error = 伺服器網路/連接錯誤：{ $raw_error }
main-servers-participant_error = 參與者斷開/協議錯誤：{ $raw_error }
main-servers-stream_error = 客戶端連接/壓縮/(反)序列化錯誤：{ $raw_error }
main-servers-database_error = 伺服器資料庫錯誤：{ $raw_error }
main-servers-persistence_error = 伺服器持久化錯誤（可能與資產/角色數據相關）：{ $raw_error }
main-servers-other_error = 伺服器一般錯誤：{ $raw_error }
main-server-rules = 伺服器有規則必須接受
main-server-rules-seen-before = 這些規則自您上次接受後已有所更改
main-credits = 製作團隊
main-credits-created_by = 創建於
main-credits-music = 音樂
main-credits-fonts = 字體
main-credits-other_art = 其他藝術
main-credits-contributors = 貢獻者
loading-tips =
    .a0 = 按 '{ $gameinput-togglelantern }' 點亮您的燈籠
    .a1 = 按 '{ $gameinput-controls }' 查看所有預設按鍵綁定
    .a2 = 您可以輸入 /say 或 /s 僅與周圍玩家聊天
    .a3 = 您可以輸入 /region 或 /r 僅與周圍幾百格的玩家聊天
    .a4 = 管理員可以使用 /build 指令進入建造模式
    .a5 = 您可以輸入 /group 或 /g 僅與隊伍內的玩家聊天
    .a6 = 若要發送私人訊息，輸入 /tell 加上玩家名稱和您的訊息
    .a7 = 在世界各地搜尋食物、寶箱和其他戰利品！
    .a8 = 背包裡滿是食物？試著將其製作成更好的食物！
    .a9 = 想知道該做什麼嗎？試試地圖上的地城！
    .a10 = 別忘了根據您的系統調整圖形設定，按 '{ $gameinput-settings }' 打開設定
    .a11 = 與他人一起玩很有趣！按 '{ $gameinput-social }' 看看誰在線
    .a12 = 按 '{ $gameinput-dance }' 跳舞，派對時間到！
    .a13 = 按 '{ $gameinput-glide }' 打開滑翔翼，征服天空
    .a14 = Caldrayne Online 還在 Pre-Alpha 階段，我們每天都在努力改進它！
    .a15 = 如果您想參與開發或提交回饋，請造訪我們的 GitHub 倉庫
    .a16 = 您可以在設定中切換是否顯示健康條上的健康數值
    .a17 = 坐在營火旁（按 '{ $gameinput-sit }' 鍵）可慢慢恢復健康
    .a18 = 需要更多背包或更好的裝甲來繼續冒險？按 '{ $gameinput-crafting }' 打開製作菜單！
    .a19 = 按 '{ $gameinput-roll }' 翻滾，翻滾可以用來更快移動並躲避敵人攻擊
    .a20 = 想知道某物品有什麼用途？在製作中搜索 'input:<item name>' 查看其用於哪些配方
    .a21 = 按 '{ $gameinput-screenshot }' 擷取螢幕截圖

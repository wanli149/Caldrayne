main-username = Username
main-server = Server
main-password = Password
main-connecting = Connecting
main-creating_world = Creating world
main-tip = Tip:
main-unbound_key_tip = unbound
main-notice =
    Welcome to Caldrayne Online Alpha!

    Before you dive into the game, please keep a few things in mind:

    - This is an early branded development build. Expect bugs, unfinished gameplay, unpolished mechanics, and missing features.

    - Caldrayne Online is built on the Veloren open-source foundation. We are preserving required upstream credit while building a distinct branded project.

    - Caldrayne Online remains open source. Please keep upstream license notices, credits, and required attribution intact when contributing.

    - The current project is still under active development, and many systems may keep historical internal `veloren` naming for compatibility.

    Thanks for taking the time to read this notice. We hope you enjoy the journey.

    ~ The Caldrayne development team
main-login_process =
    About multiplayer mode:

    Some servers may require authenticated accounts.

    If authentication is enabled, please use the account service specified by your server operator.
main-singleplayer-new = New
main-singleplayer-delete = Delete
main-singleplayer-regenerate = Regenerate
main-singleplayer-create_custom = Create Custom
main-singleplayer-seed = Seed
main-singleplayer-day_length = Day duration
main-singleplayer-random_seed = Random
main-singleplayer-size_lg = Logarithmic size
main-singleplayer-map_large_warning = Warning: Large worlds will take a long time to start for the first time.
main-singleplayer-map_large_extra_warning = This would take about the same amount of resources as generating { $count } worlds with default options.
main-singleplayer-world_name = World name
main-singleplayer-map_scale = Vertical scaling
main-singleplayer-map_erosion_quality = Erosion quality
main-singleplayer-map_shape = Shape
main-singleplayer-map_shape-circle = Circle
main-singleplayer-map_shape-square = Square
main-singleplayer-play = Play
main-singleplayer-generate_and_play = Generate & Play
menu-singleplayer-confirm_delete = Are you sure you want to delete "{ $world_name }"?
menu-singleplayer-confirm_regenerate = Are you sure you want to regenerate "{ $world_name }"?
main-login-server_not_found = Server not found.
main-login-no_ip_addr = No usable server address was resolved.
main-login-authentication_error = Authentication error on server.
main-login-internal_error = Internal error on client. Hint: The player character might have been deleted.
main-login-failed_auth_server_url_invalid = Failed to connect to authentication server.
main-login-insecure_auth_scheme = The HTTP authentication scheme is not supported. It's insecure! For development purposes, HTTP is allowed for 'localhost' or debug builds.
main-login-server_full = Server is full.
main-login-untrusted_auth_server = Authentication server not trusted.
main-login-timeout = Timeout: Server did not respond in time. Hint: the server might be currently overloaded or there are issues on the network.
main-login-server_shut_down = Server shut down.
main-login-network_error = Network error.
main-login-network_wrong_version = Mismatched server and client version. Hint: You might need to update your game client.
main-login-failed_sending_request = Request to authentication server failed.
main-login-invalid_character = The selected character is invalid.
main-login-bad_world_map_dimensions = The server sent invalid world map dimensions.
main-login-bad_world_map_image = The server sent an invalid world map image.
main-login-bad_altitude_map = The server sent an invalid altitude map.
main-login-entity_sync_failed = Failed to synchronize player entity data from the server.
main-login-client_crashed = Client crashed.
main-login-render_backend_failed = Failed to select a rendering backend. No compatible backend was found. The client currently supports Vulkan, Metal, DX12, and OpenGL. { $potential_fix } If the issue persists, please include your operating system and GPU details in your bug report.
main-login-render_backend_failed-fix-windows = Updating the graphics drivers on this system may resolve the issue.
main-login-render_backend_failed-fix-none = No additional troubleshooting advice is available right now.
main-login-render_backend_failed-fix-vulkan = Installing or updating Vulkan drivers may resolve the issue.
main-login-window_create_failed = Failed to create the game window: { $raw_error }
main-login-not_on_whitelist = You are not a member in the whitelist of the server you have attempted to join.
main-login-banned = You have been permanently banned with the following reason: { $reason }
main-login-banned_until =
   You have been temporarily banned with the following reason: { $reason }
   Until: { $end_date }
main-login-kicked = You have been kicked with the following reason: { $reason }
main-login-select_language = Select a language
main-login-client_version = Client version
main-login-server_version = Server version
main-login-client_init_failed = Client failed to initialize: { $init_fail_reason }
main-login-username_bad_characters = Username contains invalid characters! (Only alphanumeric, '_' and '-' are allowed).
main-login-username_too_long = Username is too long! Max length is: { $max_len }
main-servers-select_server = Select a server
main-servers-singleplayer_error = Failed to connect to internal server: { $sp_error }
main-servers-network_error = Server network/socket error: { $raw_error }
main-servers-participant_error = Participant disconnect/protocol error: { $raw_error }
main-servers-stream_error = Client connection/compression/(de)serialization error: { $raw_error }
main-servers-database_error = Server database error: { $raw_error }
main-servers-persistence_error = Server persistence error (Probably Asset/Character Data related): { $raw_error }
main-servers-other_error = Server general error: { $raw_error }
main-servers-world_compat_error = World loading was blocked by compatibility enforcement: { $entry } could not be used because of { $failure }. { $remediation }
main-servers-world_compat_notice = World compatibility fallback was used: { $entry } could not be loaded because of { $failure }, so this session started with a regenerated world instead. { $remediation }
main-servers-world_compat_entry-load = the selected world file
main-servers-world_compat_entry-load_legacy = the legacy world file
main-servers-world_compat_entry-load_asset = the bundled default world asset
main-servers-world_compat_entry-generic = the requested world input
main-servers-world_compat_failure-missing_input = missing input data
main-servers-world_compat_failure-parse_error = unreadable world data
main-servers-world_compat_failure-invalid_world = invalid world data
main-servers-world_compat_failure-option_mismatch = generation options that no longer match
main-servers-world_compat_failure-generic = an unknown compatibility issue
main-servers-world_compat_remediation-load = Restore the world file or regenerate this world from the world selector before starting again.
main-servers-world_compat_remediation-load_asset = Restore the default world asset and verify the installation before starting again.
main-servers-world_compat_remediation-generic = Choose another world source or regenerate the world before starting again.
main-servers-world_compat_notice_remediation-load = If you needed the previous world data, restore the original world file or recover it from backup before starting again.
main-servers-world_compat_notice_remediation-load_asset = Verify the installation if you expected the bundled default world to load unchanged.
main-servers-world_compat_notice_remediation-generic = Review the selected world source if this fallback was unexpected.
main-server-rules = This server has rules that must be accepted.
main-server-rules-seen-before = These rules have changed since the last time you accepted them.
main-credits = Credits
main-credits-created_by = created by
main-credits-music = Music
main-credits-sound = Sound
main-credits-fonts = Fonts
main-credits-other_art = Other Art
main-credits-contributors = Contributors
loading-tips =
    .a0 = Press '{ $gameinput-togglelantern }' to light your lantern.
    .a1 = Press '{ $gameinput-controls }' to see all default key bindings.
    .a2 = You can type /say or /s to only chat with players directly around you.
    .a3 = You can type /region or /r to only chat with players a couple of hundred blocks around you.
    .a4 = Admins can use the /build command to enter build mode.
    .a5 = You can type /group or /g to only chat with players in your current group.
    .a6 = To send private messages type /tell followed by a player name and your message.
    .a7 = Keep an eye out for food, chests and other loot spread all around the world!
    .a8 = Inventory filled with food? Try crafting better food from it!
    .a9 = Wondering what there is to do? Try out one of the dungeons marked on the map!
    .a10 = Don't forget to adjust the graphics for your system. Press '{ $gameinput-settings }' to open the settings.
    .a11 = Playing with others is fun! Press '{ $gameinput-social }' to see who is online.
    .a12 = Press '{ $gameinput-dance }' to dance. Party!
    .a13 = Press '{ $gameinput-glide }' to open your Glider and conquer the skies.
    .a14 = Caldrayne Online is still in Pre-Alpha. We do our best to improve it every day!
    .a15 = If you want to join development or share feedback, visit our GitHub repository.
    .a16 = You can toggle showing your amount of health on the health bar in the settings.
    .a17 = Sit near a campfire (with the '{ $gameinput-sit }' key) to slowly recover from your injuries.
    .a18 = Need more bags or better armor to continue your journey? Press '{ $gameinput-crafting }' to open the crafting menu!
    .a19 = Press '{ $gameinput-roll }' to roll. Rolling can be used to move faster and dodge enemy attacks.
    .a20 = Wondering what an item is used for? Search 'input:<item name>' in crafting to see what recipes it's used in.
    .a21 = You can take screenshots with '{ $gameinput-screenshot }'.

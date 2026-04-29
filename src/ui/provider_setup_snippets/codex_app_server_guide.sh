# Codex App Server polling (advanced). DEFERRED from F-6 master plan
# because it requires a long-running background daemon. This block is
# guidance only — Qmonster does NOT spawn the daemon today.
#
# The App Server exposes account/rateLimits/read which returns:
#   { "usedPercent": 47, "windowDurationMins": 300, "resetsAt": "..." }
#   (one entry per limit window: 5h, weekly, sometimes 1d)
#
# Manual launch (separate terminal):
#
#   codex app-server &
#
# Then poll once via stdin/stdout JSON-RPC:
#
#   echo '{"method":"initialize","id":0,"params":{"clientInfo":{"name":"qmonster","version":"1.x"}}}' | codex app-server
#   echo '{"method":"account/rateLimits/read","id":1}' | codex app-server
#
# Qmonster F-6 (when shipped) would supervise this daemon. Until then,
# rely on /status periodic invocation as documented above.

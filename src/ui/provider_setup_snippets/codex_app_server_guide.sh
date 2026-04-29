# Codex App Server polling (advanced, Qmonster-managed).
#
# Normal Qmonster flow:
#   1. Open S Settings -> Integrations.
#   2. Turn ON "Codex app-server".
#   3. Press w to write qmonster.toml.
#   4. Restart Qmonster.
#
# On startup, Qmonster spawns `codex app-server`, sends JSON-RPC
# initialize, then polls account/rateLimits/read on its own. You do not
# need to keep a separate terminal open or send JSON-RPC messages
# manually for Qmonster to receive reset-window data.
#
# The App Server exposes account/rateLimits/read which returns:
#   { "usedPercent": 47, "windowDurationMins": 300, "resetsAt": "..." }
#   (one entry per limit window: 5h, weekly, sometimes 1d)
#
# Manual probe only when diagnosing the Codex CLI outside Qmonster:
#
#   codex app-server &
#
# Then poll once via stdin/stdout JSON-RPC:
#
#   echo '{"method":"initialize","id":0,"params":{"clientInfo":{"name":"qmonster","version":"1.x"}}}' | codex app-server
#   echo '{"method":"account/rateLimits/read","id":1}' | codex app-server

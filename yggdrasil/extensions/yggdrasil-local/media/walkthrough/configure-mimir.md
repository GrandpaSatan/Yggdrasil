# Configure Mimir (Optional)

Mimir is the engram memory service. It's what makes `/memory <query>` in chat surface relevant past conversations as context.

If you don't run Mimir, the chat still works — you just won't have memory recall in the `/memory` slash command.

**What to do:**
1. In the Endpoints tab of Settings, set `Mimir URL` (default port: `9090`)
2. Click `Test` to verify connectivity
3. Click "Save Endpoints"

You can come back and configure this any time from the Settings panel.

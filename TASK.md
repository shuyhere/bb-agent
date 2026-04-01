# A2: Implement actual TypeScript plugin loading and execution

Working dir: `/tmp/bb-final/a2-plugin-loading/`
BB-Agent Rust project. Read BLUEPRINT.md and REVIEW.md for context.

## Problem
Plugin discovery (`crates/plugin-host/src/discovery.rs`) finds `.ts` files, and the host (`host.rs`) can spawn a Node process, but NO plugins are ever loaded or executed. The JSON-RPC protocol types exist but there's no host runtime that loads plugins and bridges events.

## Task: Build a working plugin host that loads and runs TS plugins

### 1. Create `crates/plugin-host/src/runtime.rs` — the plugin runtime

This is the JS file that Node runs. It loads plugins and bridges JSON-RPC:

Create the file `crates/plugin-host/js/host.js` that gets embedded or shipped:

```javascript
// Read JSON-RPC messages from stdin, line-delimited
// Each plugin exports default function(bb) where bb has:
//   bb.on(event, handler)
//   bb.registerTool(def)
//   bb.registerCommand(name, def)

const readline = require('readline');
const path = require('path');

const handlers = {};  // event -> [handler]
const tools = {};     // name -> def
const commands = {};  // name -> def

const bb = {
    on(event, handler) {
        if (!handlers[event]) handlers[event] = [];
        handlers[event].push(handler);
    },
    registerTool(def) {
        tools[def.name] = def;
        // Notify Rust side
        send({ jsonrpc: "2.0", method: "tool_registered", params: { name: def.name, description: def.description, parameters: def.parameters } });
    },
    registerCommand(name, def) {
        commands[name] = def;
        send({ jsonrpc: "2.0", method: "command_registered", params: { name, description: def.description } });
    },
};

function send(msg) {
    process.stdout.write(JSON.stringify(msg) + '\n');
}

// Load plugins from args
for (const pluginPath of process.argv.slice(2)) {
    try {
        const mod = require(path.resolve(pluginPath));
        const factory = mod.default || mod;
        if (typeof factory === 'function') factory(bb);
    } catch (e) {
        send({ jsonrpc: "2.0", method: "plugin_error", params: { path: pluginPath, error: e.message } });
    }
}

send({ jsonrpc: "2.0", method: "plugins_loaded", params: { count: process.argv.length - 2 } });

// Handle incoming events from Rust
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', async (line) => {
    try {
        const msg = JSON.parse(line);
        if (msg.method === 'event') {
            const event = msg.params;
            const eventHandlers = handlers[event.type] || [];
            let result = null;
            for (const handler of eventHandlers) {
                const r = await handler(event, {});
                if (r) result = { ...result, ...r };
                if (r?.block || r?.cancel) break;
            }
            if (msg.id !== undefined) {
                send({ jsonrpc: "2.0", id: msg.id, result: result || {} });
            }
        } else if (msg.method === 'execute_tool') {
            const { name, toolCallId, params: toolParams } = msg.params;
            const tool = tools[name];
            if (tool && tool.execute) {
                try {
                    const result = await tool.execute(toolCallId, toolParams);
                    send({ jsonrpc: "2.0", id: msg.id, result });
                } catch (e) {
                    send({ jsonrpc: "2.0", id: msg.id, error: { code: -1, message: e.message } });
                }
            } else {
                send({ jsonrpc: "2.0", id: msg.id, error: { code: -1, message: `Tool ${name} not found` } });
            }
        }
    } catch (e) {
        // Ignore parse errors
    }
});
```

### 2. Modify `crates/plugin-host/src/host.rs`

Update `PluginHost` to:
- Write `host.js` to a temp file on startup (or embed it)
- Spawn Node with `host.js` + list of plugin paths as args
- Read `plugins_loaded`, `tool_registered`, `command_registered` notifications
- Provide methods to send events and receive responses:

```rust
impl PluginHost {
    pub async fn load_plugins(plugin_paths: &[PathBuf]) -> Result<Self, ...>;
    pub async fn send_event(&mut self, event: &Event) -> Option<HookResult>;
    pub async fn execute_tool(&mut self, name: &str, tool_call_id: &str, params: Value) -> Result<ToolResult>;
    pub fn registered_tools(&self) -> &[RegisteredTool];
    pub fn registered_commands(&self) -> &[RegisteredCommand];
}
```

### 3. Integrate with the agent loop

In `crates/cli/src/interactive.rs` or `run.rs`:

On startup:
```rust
let plugins = discovery::discover_plugins(&global_dir, Some(&project_dir));
let plugin_host = if !plugins.is_empty() {
    let paths: Vec<PathBuf> = plugins.iter().map(|p| p.path.clone()).collect();
    Some(PluginHost::load_plugins(&paths).await?)
} else {
    None
};
```

Before each event emission on the EventBus, also send to plugin host:
```rust
if let Some(ref mut host) = plugin_host {
    if let Some(result) = host.send_event(&event).await {
        // Merge with bus results
    }
}
```

### 4. Test with a sample plugin

Create `~/.bb-agent/plugins/test-plugin.ts`:
```typescript
export default function(bb) {
    bb.on("session_start", (event, ctx) => {
        console.error("[test-plugin] Session started!");
    });

    bb.on("tool_call", (event, ctx) => {
        if (event.tool_name === "bash" && event.input.command?.includes("rm -rf /")) {
            return { block: true, reason: "Blocked dangerous command" };
        }
    });
}
```

### Build and test
```bash
cd /tmp/bb-final/a2-plugin-loading
cargo build && cargo test
git add -A && git commit -m "A2: implement TS plugin loading and execution"
```

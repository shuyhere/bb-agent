// BB-Agent Plugin Host Runtime
// Loaded by Node.js, bridges JSON-RPC between Rust and TS plugins.
//
// Each plugin exports: default function(bb) { ... }
// bb.on(event, handler)
// bb.registerTool(def)
// bb.registerCommand(name, def)

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
                try {
                    const r = await handler(event, {});
                    if (r) result = { ...result, ...r };
                    if (r?.block || r?.cancel) break;
                } catch (handlerErr) {
                    send({ jsonrpc: "2.0", method: "handler_error", params: { event_type: event.type, error: handlerErr.message } });
                }
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
                    send({ jsonrpc: "2.0", id: msg.id, result: result || {} });
                } catch (e) {
                    send({ jsonrpc: "2.0", id: msg.id, error: { code: -1, message: e.message } });
                }
            } else {
                send({ jsonrpc: "2.0", id: msg.id, error: { code: -1, message: `Tool ${name} not found` } });
            }
        }
    } catch (e) {
        // Ignore parse errors on stdin
    }
});

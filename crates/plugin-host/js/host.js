// BB-Agent Plugin Host Runtime
// Loaded by Node.js, bridges JSON-RPC between Rust and TS plugins.
//
// Each plugin exports: default function(bb) { ... }
// bb.on(event, handler)
// bb.registerTool(def)
// bb.registerCommand(name, def)

const readline = require('readline');
const path = require('path');
const { execFileSync } = require('child_process');

function resolveJiti() {
    const candidates = [];
    if (process.env.BB_JITI_PATH) candidates.push(process.env.BB_JITI_PATH);

    try {
        const globalRoot = execFileSync('npm', ['root', '-g'], { encoding: 'utf8' }).trim();
        if (globalRoot) {
            candidates.push(path.join(globalRoot, 'jiti'));
            candidates.push(path.join(globalRoot, '@mariozechner', 'pi-coding-agent', 'node_modules', 'jiti'));
        }
    } catch (_error) {
        // Ignore lookup failures and fall back to normal resolution.
    }

    candidates.push('jiti');
    for (const candidate of candidates) {
        try {
            return require(candidate);
        } catch (_error) {
            // Try next candidate.
        }
    }
    return null;
}

function loadPluginModule(pluginPath) {
    const resolvedPath = path.resolve(pluginPath);
    const ext = path.extname(resolvedPath);

    if (ext === '.ts' || ext === '.mts' || ext === '.cts') {
        const jiti = resolveJiti();
        if (!jiti) {
            throw new Error(`TypeScript extension loading requires jiti. Could not load ${pluginPath}`);
        }
        const loader = typeof jiti === 'function' ? jiti(__filename, { interopDefault: true }) : jiti;
        return loader(resolvedPath);
    }

    return require(resolvedPath);
}

const handlers = {};  // event -> [handler]
const tools = {};     // name -> def
const commands = {};  // name -> def

// ── UI request/response plumbing ─────────────────────────────────
const pendingUiRequests = {};  // id -> resolve function
let uiRequestCounter = 0;

function requestUI(method, params) {
    const id = `ui_${uiRequestCounter++}`;
    send({ jsonrpc: "2.0", method: "ui_request", params: { id, method, ...params } });
    return new Promise(resolve => {
        pendingUiRequests[id] = resolve;
    });
}

function fireAndForgetUI(method, params) {
    const id = `ui_${uiRequestCounter++}`;
    send({ jsonrpc: "2.0", method: "ui_request", params: { id, method, ...params } });
}

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

function buildContext(raw) {
    const context = raw || {};
    const entries = context.session_entries || [];
    const branch = context.session_branch || entries;
    const labels = context.labels || {};
    const hasUI = !!context.has_ui || !!context.hasUI;
    return {
        cwd: context.cwd || process.cwd(),
        hasUI,
        signal: undefined,
        sessionManager: {
            getEntries: () => entries,
            getBranch: () => branch,
            getLeafId: () => context.leaf_id || context.leafId || null,
            getSessionFile: () => context.session_file || context.sessionFile || undefined,
            getLabel: (id) => labels[id] ?? undefined,
            getSessionId: () => context.session_id || context.sessionId || undefined,
            getSessionName: () => context.session_name || context.sessionName || undefined,
        },
        ui: {
            notify: (message, notifyType) => {
                fireAndForgetUI('notify', { message: String(message), notifyType: notifyType || 'info' });
            },
            setStatus: (statusKey, statusText) => {
                fireAndForgetUI('setStatus', { statusKey: String(statusKey), statusText: statusText != null ? String(statusText) : undefined });
            },
            setTitle: (title) => {
                fireAndForgetUI('setTitle', { title: String(title) });
            },
            setEditorText: (text) => {
                fireAndForgetUI('set_editor_text', { text: String(text) });
            },
            setWidget: (widgetKey, widgetLines, options) => {
                const placement = options?.placement || 'aboveEditor';
                const lines = Array.isArray(widgetLines) ? widgetLines.map(String) : undefined;
                fireAndForgetUI('setWidget', { widgetKey: String(widgetKey), widgetLines: lines, widgetPlacement: placement });
            },
            select: async (title, options, timeout) => {
                const resp = await requestUI('select', { title: String(title), options: (options || []).map(String), timeout });
                if (resp.cancelled) return undefined;
                return resp.value;
            },
            confirm: async (title, message, timeout) => {
                const resp = await requestUI('confirm', { title: String(title), message: String(message || ''), timeout });
                return !!resp.confirmed;
            },
            input: async (title, placeholder) => {
                const resp = await requestUI('input', { title: String(title), placeholder: placeholder != null ? String(placeholder) : undefined });
                if (resp.cancelled) return undefined;
                return resp.value;
            },
            editor: async (title, prefill) => {
                const resp = await requestUI('editor', { title: String(title), prefill: prefill != null ? String(prefill) : undefined });
                if (resp.cancelled) return undefined;
                return resp.value;
            },
            custom: () => undefined,
            setWorkingMessage: () => {},
            setFooter: () => {},
            setHeader: () => {},
            setEditorComponent: () => {},
            setToolsExpanded: () => {},
            getEditorText: () => '',
            getToolsExpanded: () => false,
            pasteToEditor: (text) => {
                fireAndForgetUI('set_editor_text', { text: String(text) });
            },
            getAllThemes: () => [],
            getTheme: () => undefined,
            setTheme: () => ({ success: false, error: 'not supported' }),
        },
        getSystemPrompt: () => context.system_prompt || context.systemPrompt || '',
        shutdown: () => undefined,
    };
}

for (const pluginPath of process.argv.slice(2)) {
    try {
        const mod = loadPluginModule(pluginPath);
        const factory = mod.default || mod;
        if (typeof factory === 'function') factory(bb);
    } catch (e) {
        send({ jsonrpc: "2.0", method: "plugin_error", params: { path: pluginPath, error: e.message } });
    }
}

send({ jsonrpc: "2.0", method: "plugins_loaded", params: { count: process.argv.length - 2 } });

const rl = readline.createInterface({ input: process.stdin });
rl.on('line', async (line) => {
    try {
        const msg = JSON.parse(line);

        // Handle UI responses from the Rust host
        if (msg.method === 'ui_response') {
            const params = msg.params || {};
            const resolver = pendingUiRequests[params.id];
            if (resolver) {
                delete pendingUiRequests[params.id];
                resolver(params.data || params);
            }
            return;
        }

        if (msg.method === 'event') {
            const event = msg.params.event;
            const ctx = buildContext(msg.params.context);
            const eventHandlers = handlers[event.type] || [];
            let result = {};
            for (const handler of eventHandlers) {
                try {
                    const r = await handler(event, ctx);
                    if (!r) continue;

                    result = { ...result, ...r };

                    if (event.type === 'input') {
                        if (typeof r.text === 'string') {
                            event.text = r.text;
                            result.text = r.text;
                        }
                        if (r.action === 'handled') break;
                    } else if (event.type === 'before_agent_start') {
                        if (typeof r.system_prompt === 'string') {
                            event.system_prompt = r.system_prompt;
                            result.system_prompt = r.system_prompt;
                        }
                    } else if (event.type === 'tool_result') {
                        if (r.content !== undefined) {
                            event.content = r.content;
                            result.content = r.content;
                        }
                        if (r.details !== undefined) {
                            event.details = r.details;
                            result.details = r.details;
                        }
                        if (r.isError !== undefined) {
                            event.is_error = r.isError;
                            result.is_error = r.isError;
                        }
                        if (r.is_error !== undefined) {
                            event.is_error = r.is_error;
                            result.is_error = r.is_error;
                        }
                    }

                    if (r?.block || r?.cancel) break;
                } catch (handlerErr) {
                    send({ jsonrpc: "2.0", method: "handler_error", params: { event_type: event.type, error: handlerErr.message } });
                }
            }

            if (event.type === 'tool_call') {
                result.input = event.input;
            } else if (event.type === 'tool_result') {
                result.content = event.content;
                if (event.details !== undefined) result.details = event.details;
                if (event.is_error !== undefined) result.is_error = event.is_error;
            } else if (event.type === 'before_agent_start') {
                result.system_prompt = event.system_prompt;
            } else if (event.type === 'input' && typeof event.text === 'string') {
                result.text = event.text;
            }

            if (msg.id !== undefined) {
                send({ jsonrpc: "2.0", id: msg.id, result });
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
        } else if (msg.method === 'execute_command') {
            const { name, args, context } = msg.params;
            const command = commands[name];
            if (command && command.handler) {
                try {
                    const result = await command.handler(args || '', buildContext(context));
                    send({ jsonrpc: "2.0", id: msg.id, result: result ?? null });
                } catch (e) {
                    send({ jsonrpc: "2.0", id: msg.id, error: { code: -1, message: e.message } });
                }
            } else {
                send({ jsonrpc: "2.0", id: msg.id, error: { code: -1, message: `Command ${name} not found` } });
            }
        }
    } catch (_e) {
        // Ignore parse errors on stdin
    }
});

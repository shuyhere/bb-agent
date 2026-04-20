// bb-shape: Anything-to-Agent
//
// Registers a single /shape slash command that opens structured menus and
// prompt flows in the BB-Agent TUI.
//
// UX:
//   /shape                → menu
//   pick New              → local prompt: Resources
//   submit resources      → local prompt: Identity
//   submit identity       → confirm menu
//   confirm build         → hand off to the shape skill
//   /shape load           → local prompt: agent name or id
//   /shape load <query>   → resolve by exact/partial name or id
//
// Detailed docs remain available via `/shape help [sub]`.

const fs = require('fs');
const os = require('os');
const path = require('path');

const SUBCOMMANDS = [
    {
        key: 'new',
        label: '✨ New',
        detail: 'Create a new agent from URLs, docs, or descriptions',
    },
    {
        key: 'list',
        label: '📋 List',
        detail: 'Browse & activate existing agents',
    },
    {
        key: 'organize',
        label: '🗂  Organize',
        detail: 'Rename, remove, or inspect agents',
    },
    {
        key: 'load',
        label: '🔄 Load',
        detail: 'Quick-activate an agent by name or ID',
    },
];

function encodeState(obj) {
    return Buffer.from(JSON.stringify(obj), 'utf8')
        .toString('base64')
        .replace(/\+/g, '-')
        .replace(/\//g, '_')
        .replace(/=+$/g, '');
}

function decodeState(token) {
    try {
        const normalized = token.replace(/-/g, '+').replace(/_/g, '/');
        const pad = normalized.length % 4 === 0 ? '' : '='.repeat(4 - (normalized.length % 4));
        return JSON.parse(Buffer.from(normalized + pad, 'base64').toString('utf8'));
    } catch (_e) {
        return null;
    }
}

function parseArgs(raw) {
    const text = (raw || '').trim();
    if (text === '') return { sub: null, rest: '' };

    if (text.startsWith('__resume ')) {
        const after = text.slice('__resume '.length);
        const sep = after.indexOf(' -- ');
        if (sep >= 0) {
            return {
                sub: '__resume',
                token: after.slice(0, sep).trim(),
                input: after.slice(sep + 4),
            };
        }
        const m = after.match(/^(\S+)\s*(.*)$/);
        return {
            sub: '__resume',
            token: m ? m[1] : '',
            input: m ? m[2] : '',
        };
    }

    const match = text.match(/^(\S+)\s*(.*)$/);
    if (!match) return { sub: null, rest: '' };

    const head = match[1].toLowerCase();
    const rest = match[2] || '';

    if (head === 'help') {
        return { sub: 'help', rest };
    }

    if (head === 'load') {
        return { sub: 'load', rest };
    }

    if (head === 'activate') {
        return { sub: 'activate', rest };
    }

    if (/^\d+$/.test(head)) {
        const idx = parseInt(head, 10) - 1;
        if (idx >= 0 && idx < SUBCOMMANDS.length) {
            return { sub: SUBCOMMANDS[idx].key, rest };
        }
        return { sub: '__unknown', rest: text };
    }

    const known = SUBCOMMANDS.find((c) => c.key === head);
    if (known) return { sub: known.key, rest };

    return { sub: '__unknown', rest: text };
}

function registryPath() {
    return path.join(os.homedir(), '.bb-agent', 'agents', 'registry.json');
}

function readRegistry() {
    const file = registryPath();
    if (!fs.existsSync(file)) {
        return { version: 1, agents: [] };
    }
    try {
        const parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
        if (!parsed || typeof parsed !== 'object') {
            return { version: 1, agents: [] };
        }
        if (!Array.isArray(parsed.agents)) {
            return { version: parsed.version || 1, agents: [] };
        }
        return parsed;
    } catch (_err) {
        return { version: 1, agents: [] };
    }
}

function normalizeText(value) {
    return String(value || '').trim();
}

function activationIdForAgent(agent) {
    return normalizeText(agent.id) || normalizeText(agent.path);
}

function formatAgentMenuLabel(agent) {
    return normalizeText(agent.name) || normalizeText(agent.id) || normalizeText(agent.path) || 'Unnamed Agent';
}

function formatAgentMenuDetail(agent) {
    const parts = [];
    if (agent.role) parts.push(agent.role);
    if (agent.source_summary) parts.push(agent.source_summary);
    if (agent.created_at) parts.push(String(agent.created_at).slice(0, 10));
    return parts.join(' • ');
}

function activateAgent(agentId, note) {
    const result = {
        activateAgent: {
            id: agentId,
        },
    };
    if (note) {
        result.message = note;
    }
    return result;
}

function listAgentsMenu() {
    const registry = readRegistry();
    const agents = Array.isArray(registry.agents) ? registry.agents : [];
    const items = agents
        .map((agent) => {
            const activationId = activationIdForAgent(agent);
            if (!activationId) return null;
            return {
                label: formatAgentMenuLabel(agent),
                detail: formatAgentMenuDetail(agent) || activationId,
                value: `activate ${activationId}`,
            };
        })
        .filter(Boolean);

    if (items.length === 0) {
        return {
            message: 'No agents yet. Use /shape new to create one.',
        };
    }

    return {
        menu: {
            title: '🔷 Shape — Your Agents',
            items,
        },
    };
}

function menuResponse() {
    return {
        menu: {
            title: '🔷 Shape — Anything-to-Agent',
            items: SUBCOMMANDS.map((c) => ({
                label: c.label,
                detail: c.detail,
                value: c.key,
            })),
        },
    };
}

function promptResponse({ command = 'shape', title, lines, inputLabel, inputPlaceholder, state }) {
    return {
        prompt: {
            title,
            lines,
            inputLabel,
            inputPlaceholder,
            resume: encodeState({ command, ...state }),
        },
    };
}

function confirmMenu(title, items) {
    return {
        menu: {
            title,
            items,
        },
    };
}

function buildWizardLines(error, baseLines) {
    if (!error) {
        return baseLines;
    }
    return [error, '', ...baseLines];
}

function startNewWizard({ resources = '', identity = '', error } = {}) {
    return promptResponse({
        title: '🔷 Shape — New Agent',
        lines: buildWizardLines(error, [
            'Give me your resources.',
            '',
            'Separate multiple sources with commas.',
            'Supported:',
            '  🌐 URLs       — https://docs.example.com',
            '  📄 Documents  — ./handbook.pdf, ~/notes.md',
            '  💬 Text       — "A pizza restaurant in Brooklyn..."',
        ]),
        inputLabel: 'Resources',
        inputPlaceholder: resources || 'https://mystore.com, ./faq.md, "Free returns 30d"',
        state: { flow: 'new', step: 'resources', identity },
    });
}

function startIdentityPrompt({ resources, identity = '', error } = {}) {
    return promptResponse({
        title: '🔷 Shape — New Agent',
        lines: buildWizardLines(error, [
            'Now describe the agent in one or two sentences.',
            '',
            'Example:',
            '  A friendly customer support agent for my online store, helping',
            '  shoppers with orders, returns, and product questions.',
        ]),
        inputLabel: 'Identity',
        inputPlaceholder: identity || 'A friendly support agent for my store...',
        state: {
            flow: 'new',
            step: 'identity',
            resources: normalizeText(resources),
        },
    });
}

function startLoadWizard({ query = '', error } = {}) {
    return promptResponse({
        title: '🔷 Shape — Load Agent',
        lines: buildWizardLines(error, [
            'Enter an agent name or ID to activate.',
            '',
            'Examples:',
            '  mystore support',
            '  mystore-support-a1b2c3',
        ]),
        inputLabel: 'Agent name or ID',
        inputPlaceholder: query || 'MyStore Support',
        state: {
            flow: 'load',
            step: 'query',
        },
    });
}

function scoreAgentMatch(agent, query) {
    const q = normalizeText(query).toLowerCase();
    if (!q) return 0;

    const id = normalizeText(agent.id).toLowerCase();
    const pathValue = normalizeText(agent.path).toLowerCase();
    const name = normalizeText(agent.name).toLowerCase();
    const role = normalizeText(agent.role).toLowerCase();
    const source = normalizeText(agent.source_summary).toLowerCase();

    if (id && id === q) return 500;
    if (pathValue && pathValue === q) return 480;
    if (name && name === q) return 450;

    let score = 0;
    if (id.startsWith(q)) score = Math.max(score, 400);
    if (pathValue.startsWith(q)) score = Math.max(score, 380);
    if (name.startsWith(q)) score = Math.max(score, 350);
    if (id.includes(q)) score = Math.max(score, 300);
    if (pathValue.includes(q)) score = Math.max(score, 280);
    if (name.includes(q)) score = Math.max(score, 250);
    if (role.includes(q)) score = Math.max(score, 120);
    if (source.includes(q)) score = Math.max(score, 100);
    return score;
}

function findAgents(agents, query) {
    const normalized = normalizeText(query);
    if (!normalized) return [];

    return (Array.isArray(agents) ? agents : [])
        .map((agent) => ({ agent, score: scoreAgentMatch(agent, normalized) }))
        .filter((item) => item.score > 0)
        .sort((a, b) => {
            if (b.score !== a.score) return b.score - a.score;
            return formatAgentMenuLabel(a.agent).localeCompare(formatAgentMenuLabel(b.agent));
        })
        .map((item) => item.agent);
}

function resolveLoadQuery(query) {
    const normalized = normalizeText(query);
    if (!normalized) {
        return startLoadWizard({ error: 'Please enter an agent name or ID.' });
    }

    const registry = readRegistry();
    const agents = Array.isArray(registry.agents) ? registry.agents : [];
    const matches = findAgents(agents, normalized);

    if (matches.length === 0) {
        return {
            message: `No agent found matching "${normalized}". Use /shape list to see all agents.`,
        };
    }

    if (matches.length === 1) {
        const agent = matches[0];
        const activationId = activationIdForAgent(agent);
        if (!activationId) {
            return {
                message: `Found "${formatAgentMenuLabel(agent)}", but it has no usable id/path in the registry.`,
            };
        }
        return activateAgent(
            activationId,
            `Activated ${formatAgentMenuLabel(agent)} via /shape load ${normalized}`,
        );
    }

    return {
        menu: {
            title: `🔷 Shape — Matches for "${normalized}"`,
            items: matches
                .map((agent) => {
                    const activationId = activationIdForAgent(agent);
                    if (!activationId) return null;
                    return {
                        label: formatAgentMenuLabel(agent),
                        detail: formatAgentMenuDetail(agent) || activationId,
                        value: `activate ${activationId}`,
                    };
                })
                .filter(Boolean),
        },
    };
}

function helpText(sub) {
    switch ((sub || '').toLowerCase()) {
        case 'new':
            return [
                '✨ /shape new — Create a new agent',
                '',
                'This opens a guided local flow:',
                '  1. Resources page  → enter URLs, files, or descriptions',
                '  2. Identity page   → describe the role and audience',
                '  3. Confirm menu    → build or go back',
                '  4. Skill hand-off  → SKILL.md executes the real build',
                '',
                'Example resources:',
                '  https://mystore.com, https://mystore.com/faq, "Free returns 30d"',
            ].join('\n');
        case 'list':
            return [
                '📋 /shape list — Browse & activate existing agents',
                '',
                'Reads ~/.bb-agent/agents/registry.json and shows every shaped agent.',
                'Picking one activates it in the current session.',
            ].join('\n');
        case 'organize':
            return [
                '🗂  /shape organize — Rename, remove, or inspect agents',
                '',
                'Lets you manage agents in ~/.bb-agent/agents/: rename, remove,',
                'or view details.',
            ].join('\n');
        case 'load':
            return [
                '🔄 /shape load <name-or-id> — Quick-activate an existing agent',
                '',
                'Searches ~/.bb-agent/agents/registry.json by exact or partial',
                'agent name, id, or saved path and activates the best match.',
                '',
                'Examples:',
                '  /shape load mystore',
                '  /shape load mystore-support-a1b2c3',
            ].join('\n');
        default:
            return [
                'Shape help',
                '',
                'Commands:',
                '  /shape                Open the Shape menu',
                '  /shape new            Start guided new-agent wizard',
                '  /shape list           Browse & activate existing agents',
                '  /shape organize       Rename, remove, or inspect agents',
                '  /shape load <query>   Quick-activate an agent by name or id',
                '  /shape help <sub>     Show detailed help for a sub-command',
            ].join('\n');
    }
}

function kickoffOrganize() {
    return {
        dispatch: {
            note: '▶ Starting Shape: Organize Agents',
            prompt: [
                'Please run the Shape "Organize" flow now.',
                '',
                'Instructions:',
                '1. Read `skills/shape/SKILL.md` and follow the section titled "## Organize".',
                '2. Start by loading `~/.bb-agent/agents/registry.json`.',
                '3. Present the organize menu (Rename / Remove / View details / Done).',
            ].join('\n'),
        },
    };
}

function kickoffBuild(resources, identity) {
    const note = [
        '▶ Starting Shape: New Agent Build',
        `  Resources: ${resources}`,
        `  Identity:  ${identity}`,
    ].join('\n');
    const body = [
        'Please run the Shape "New Agent" build flow now.',
        '',
        'Use these collected values and do NOT ask the user for them again unless',
        'something is actually missing or invalid:',
        '',
        `  Resources: ${resources}`,
        `  Identity:  ${identity}`,
        '',
        'Instructions:',
        '1. Read `skills/shape/SKILL.md` and follow the section titled "## New".',
        '2. Skip Step 1 (Collect Resources) and Step 3 (Draft Identity) — both are already provided above.',
        '3. Go straight to Step 2 (Ingest All Sources):',
        '   - For each URL in Resources, deep-crawl using `references/crawl_strategy.md`.',
        '   - For each local path, read it.',
        '   - For free-text, parse it as raw context.',
        '4. Show the ingestion summary, then proceed to Step 4 (Quick Confirmation).',
        '5. Once confirmed, execute Step 5 (Build) and Step 6 (Done).',
        '',
        'Use your real file/read/write/web_fetch tools so the user can see progress.',
    ].join('\n');
    return { dispatch: { note, prompt: body } };
}

function handleResume(token, input) {
    const state = decodeState(token);
    if (!state || state.command !== 'shape') {
        return { message: 'Invalid or expired Shape wizard state. Use /shape to start again.' };
    }

    const value = normalizeText(input);

    if (state.flow === 'new' && state.step === 'resources') {
        if (!value) {
            return startNewWizard({
                resources: '',
                identity: state.identity || '',
                error: 'Please enter at least one resource before continuing.',
            });
        }
        return startIdentityPrompt({
            resources: value,
            identity: state.identity || '',
        });
    }

    if (state.flow === 'new' && state.step === 'identity') {
        if (!value) {
            return startIdentityPrompt({
                resources: state.resources || '',
                identity: '',
                error: 'Please describe the agent identity before continuing.',
            });
        }
        const confirmState = encodeState({
            command: 'shape',
            flow: 'new',
            step: 'confirm',
            resources: state.resources || '',
            identity: value,
        });
        return confirmMenu('🔷 Shape — Confirm Build', [
            {
                label: '✅ Build agent',
                detail: 'Use the collected resources + identity and start the build',
                value: `__resume ${confirmState} yes`,
            },
            {
                label: '✏️  Edit resources',
                detail: 'Go back to the resources step',
                value: `__resume ${confirmState} edit-resources`,
            },
            {
                label: '✏️  Edit identity',
                detail: 'Go back to the identity step',
                value: `__resume ${confirmState} edit-identity`,
            },
            {
                label: '✖ Cancel',
                detail: 'Exit the wizard without building',
                value: `__resume ${confirmState} cancel`,
            },
        ]);
    }

    if (state.flow === 'new' && state.step === 'confirm') {
        if (value === 'yes') {
            return kickoffBuild(state.resources || '', state.identity || '');
        }
        if (value === 'edit-resources') {
            return startNewWizard({
                resources: state.resources || '',
                identity: state.identity || '',
            });
        }
        if (value === 'edit-identity') {
            return startIdentityPrompt({
                resources: state.resources || '',
                identity: state.identity || '',
            });
        }
        return { message: 'Shape wizard cancelled.' };
    }

    if (state.flow === 'load' && state.step === 'query') {
        if (!value) {
            return startLoadWizard({ error: 'Please enter an agent name or ID.' });
        }
        return resolveLoadQuery(value);
    }

    return { message: 'Unknown Shape wizard state. Use /shape to start again.' };
}

function registerShapeExtension(bb) {
    bb.registerCommand('shape', {
        description:
            'Anything-to-Agent: build, list, load, or organize specialized agents from URLs, docs, or text.',
        handler: async (args, _ctx) => {
            const parsed = parseArgs(args);

            if (!parsed.sub) {
                return menuResponse();
            }

            if (parsed.sub === '__resume') {
                return handleResume(parsed.token, parsed.input);
            }

            if (parsed.sub === 'help') {
                const topic = (parsed.rest || '').trim().split(/\s+/)[0] || '';
                return { message: helpText(topic) };
            }

            if (parsed.sub === 'new') {
                return startNewWizard();
            }
            if (parsed.sub === 'list') {
                return listAgentsMenu();
            }
            if (parsed.sub === 'organize') {
                return kickoffOrganize();
            }
            if (parsed.sub === 'activate') {
                const target = normalizeText(parsed.rest);
                if (!target) {
                    return { message: 'No agent selected. Use /shape list and choose an agent.' };
                }
                return activateAgent(target);
            }
            if (parsed.sub === 'load') {
                const target = normalizeText(parsed.rest);
                if (!target) {
                    return startLoadWizard();
                }
                return resolveLoadQuery(target);
            }
            if (parsed.sub === '__unknown') {
                return { message: 'Unknown /shape command. Use /shape or /shape help.' };
            }

            return { message: 'Unknown /shape command. Use /shape or /shape help.' };
        },
    });
}

module.exports = registerShapeExtension;
module.exports._internal = {
    activationIdForAgent,
    decodeState,
    encodeState,
    findAgents,
    handleResume,
    helpText,
    listAgentsMenu,
    parseArgs,
    readRegistry,
    resolveLoadQuery,
    scoreAgentMatch,
    startLoadWizard,
    startNewWizard,
    startIdentityPrompt,
};

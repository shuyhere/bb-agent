const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const shape = require('../src/index.js');
const internal = shape._internal;

function withTempHome(fn) {
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'bb-shape-test-'));
    const prevHome = process.env.HOME;
    process.env.HOME = tempRoot;
    try {
        return fn(tempRoot);
    } finally {
        process.env.HOME = prevHome;
        fs.rmSync(tempRoot, { recursive: true, force: true });
    }
}

function writeRegistry(homeDir, agents) {
    const agentsDir = path.join(homeDir, '.bb-agent', 'agents');
    fs.mkdirSync(agentsDir, { recursive: true });
    fs.writeFileSync(
        path.join(agentsDir, 'registry.json'),
        JSON.stringify({ version: 1, agents }, null, 2),
        'utf8',
    );
}

test('numeric shortcut 4 maps to load and unknown subcommands stay unknown', () => {
    assert.deepEqual(internal.parseArgs('4'), { sub: 'load', rest: '' });
    assert.deepEqual(internal.parseArgs('nonsense'), { sub: '__unknown', rest: 'nonsense' });
});

test('load query resolves exact id to activation result', () => {
    withTempHome((homeDir) => {
        writeRegistry(homeDir, [
            {
                id: 'mystore-support-a1b2c3',
                path: 'mystore-support-a1b2c3',
                name: 'MyStore Support',
                role: 'Customer Support Agent',
            },
        ]);

        const result = internal.resolveLoadQuery('mystore-support-a1b2c3');
        assert.equal(result.activateAgent.id, 'mystore-support-a1b2c3');
        assert.match(result.message, /Activated MyStore Support/);
    });
});

test('load query opens a menu when multiple agents match', () => {
    withTempHome((homeDir) => {
        writeRegistry(homeDir, [
            {
                id: 'mystore-support-a1b2c3',
                path: 'mystore-support-a1b2c3',
                name: 'MyStore Support',
            },
            {
                id: 'mystore-sales-b2c3d4',
                path: 'mystore-sales-b2c3d4',
                name: 'MyStore Sales',
            },
        ]);

        const result = internal.resolveLoadQuery('mystore');
        assert.equal(result.menu.title, '🔷 Shape — Matches for "mystore"');
        assert.equal(result.menu.items.length, 2);
        assert.equal(result.menu.items[0].value.startsWith('activate '), true);
    });
});

test('resource step rejects empty input and preserves prior identity', () => {
    const token = internal.encodeState({
        command: 'shape',
        flow: 'new',
        step: 'resources',
        identity: 'Existing identity',
    });

    const result = internal.handleResume(token, '   ');
    assert.equal(result.prompt.inputLabel, 'Resources');
    assert.match(result.prompt.lines[0], /Please enter at least one resource/);

    const decoded = internal.decodeState(result.prompt.resume);
    assert.equal(decoded.identity, 'Existing identity');
});

test('load without query opens the load prompt', () => {
    const result = internal.startLoadWizard();
    assert.equal(result.prompt.inputLabel, 'Agent name or ID');
    const decoded = internal.decodeState(result.prompt.resume);
    assert.equal(decoded.flow, 'load');
    assert.equal(decoded.step, 'query');
});

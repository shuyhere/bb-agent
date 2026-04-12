import test from 'node:test';
import assert from 'node:assert/strict';
import * as pi from '/home/shuyhere/projects/tako/node_modules/@mariozechner/pi-coding-agent/dist/core/compaction/compaction.js';

const settings = { enabled: true, reserveTokens: 16384, keepRecentTokens: 20000 };

function bbEstimateContextTokens(messages) {
  function calc(usage) {
    return usage.total_tokens > 0
      ? usage.total_tokens
      : usage.input + usage.output + usage.cache_read + usage.cache_write;
  }

  function estimateText(text) {
    return Math.ceil(text.length / 4);
  }

  function estimateMessage(message) {
    switch (message.role) {
      case 'user':
        return message.content.reduce((sum, block) => sum + (block.type === 'text' ? estimateText(block.text) : 1200), 0);
      case 'assistant':
        return message.content.reduce((sum, block) => {
          if (block.type === 'text') return sum + estimateText(block.text);
          if (block.type === 'thinking') return sum + estimateText(block.thinking);
          if (block.type === 'toolCall') return sum + estimateText(block.name) + estimateText(JSON.stringify(block.arguments ?? {}));
          return sum;
        }, 0);
      case 'toolResult':
      case 'custom':
        return message.content.reduce((sum, block) => sum + (block.type === 'text' ? estimateText(block.text) : 1200), 0);
      case 'bashExecution':
        return estimateText(message.command) + estimateText(message.output);
      case 'branchSummary':
      case 'compactionSummary':
        return estimateText(message.summary);
      default:
        return 0;
    }
  }

  const lastUsageIndex = [...messages.keys()].reverse().find((i) => {
    const m = messages[i];
    return m.role === 'assistant' && m.stopReason !== 'aborted' && m.stopReason !== 'error' && calc(m.usage) > 0;
  });

  if (lastUsageIndex == null) {
    const trailingTokens = messages.reduce((sum, m) => sum + estimateMessage(m), 0);
    return { tokens: trailingTokens, usageTokens: 0, trailingTokens, lastUsageIndex: null };
  }

  const usageTokens = calc(messages[lastUsageIndex].usage);
  const trailingTokens = messages.slice(lastUsageIndex + 1).reduce((sum, m) => sum + estimateMessage(m), 0);
  return { tokens: usageTokens + trailingTokens, usageTokens, trailingTokens, lastUsageIndex };
}

test('bb estimator matches installed pi on shared fixtures', () => {
  const fixtures = [
    [
      {
        role: 'assistant',
        content: [{ type: 'text', text: 'done' }],
        provider: 'test',
        model: 'test',
        usage: { input: 100, output: 20, cacheRead: 10, cacheWrite: 5, totalTokens: 0 },
        stopReason: 'stop',
        timestamp: Date.now(),
      },
      { role: 'user', content: [{ type: 'text', text: '12345678' }], timestamp: Date.now() },
    ],
    [
      {
        role: 'assistant',
        content: [{ type: 'text', text: 'aborted' }],
        provider: 'test',
        model: 'test',
        usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 500 },
        stopReason: 'aborted',
        timestamp: Date.now(),
      },
      { role: 'user', content: [{ type: 'text', text: '12345678' }], timestamp: Date.now() },
    ],
    [
      {
        role: 'assistant',
        content: [
          { type: 'text', text: 'hello' },
          { type: 'thinking', thinking: 'reasoning text' },
          { type: 'toolCall', id: '1', name: 'read', arguments: { path: 'src/main.rs' } },
        ],
        provider: 'test',
        model: 'test',
        usage: { input: 300, output: 40, cacheRead: 50, cacheWrite: 0, totalTokens: 0 },
        stopReason: 'stop',
        timestamp: Date.now(),
      },
      {
        role: 'toolResult',
        toolCallId: '1',
        toolName: 'read',
        content: [{ type: 'text', text: 'fn main() {}' }],
        isError: false,
        timestamp: Date.now(),
      },
      {
        role: 'bashExecution',
        command: 'ls -la',
        output: 'file1\nfile2\n',
        cancelled: false,
        truncated: false,
        timestamp: Date.now(),
      },
    ],
  ];

  for (const messages of fixtures) {
    const piEstimate = pi.estimateContextTokens(messages);
    const bbEstimate = bbEstimateContextTokens(
      JSON.parse(
        JSON.stringify(messages)
          .replace(/cacheRead/g, 'cache_read')
          .replace(/cacheWrite/g, 'cache_write')
          .replace(/totalTokens/g, 'total_tokens')
          .replace(/stopReason/g, 'stopReason')
      )
    );

    assert.deepEqual(bbEstimate, {
      tokens: piEstimate.tokens,
      usageTokens: piEstimate.usageTokens,
      trailingTokens: piEstimate.trailingTokens,
      lastUsageIndex: piEstimate.lastUsageIndex,
    });

    const contextWindow = 128000;
    assert.equal(
      bbEstimate.tokens > contextWindow - settings.reserveTokens,
      pi.shouldCompact(piEstimate.tokens, contextWindow, settings)
    );
  }
});

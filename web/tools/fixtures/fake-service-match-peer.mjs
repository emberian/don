// Deterministic parser fixture for web/local-match.mjs. The real Chrome smoke points at
// the compiled Rust service-match-peer; this script only mutation-tests the bounded Node owner.

import { createInterface } from 'node:readline';

const [mode, ...args] = process.argv.slice(2);
let seed = 0x89abcdef;
if (mode === 'host') {
  const at = args.indexOf('--seed');
  if (at >= 0) seed = Number(args[at + 1]);
  console.log('SERVICE 127.0.0.1:41991 lobby-fixture');
} else if (mode !== 'join') {
  process.exitCode = 2;
  process.exit();
}
if (mode === 'join' && process.env.DON_FAKE_MATCHSTART_MISMATCH === '1') seed++;

const epoch = 305419896;
for (const event of ['directory_started', 'match_confirmed']) {
  console.log(JSON.stringify({
    event, lobby: 'lobby-fixture', reference: 'lobby-fixture', epoch, seed,
  }));
}
const id = mode === 'host' ? 101 : 202;
if (!args.includes('--relay')) {
  console.log(JSON.stringify({ event: 'turn', stamp: 0, packages: 2, hash: '0123456789abcdef' }));
  console.log(JSON.stringify({ event: 'done', id, hash: '0123456789abcdef' }));
} else {
  console.log(JSON.stringify({ event: 'relay_ready', id, nextStamp: 0 }));
  const input = createInterface({ input: process.stdin, terminal: false });
  let nextStamp = 0;
  input.on('line', (line) => {
    if (line === 'QUIT') {
      input.close();
      return;
    }
    if (line !== `TURN ${nextStamp} 0c`) {
      console.error(`unexpected fake relay input ${JSON.stringify(line)}`);
      process.exitCode = 2;
      input.close();
      return;
    }
    const ordered = [0, 1].map((play) => ({
      stamp: nextStamp, play, payload: '0c',
    }));
    const hash = mode === 'join' && process.env.DON_FAKE_TURN_MISMATCH === '1'
      ? 'fedcba9876543210' : '0123456789abcdef';
    console.log(JSON.stringify({
      event: 'turn', stamp: nextStamp, packages: 2, ordered, hash,
    }));
    console.log(JSON.stringify({
      event: 'turn_complete', stamp: nextStamp, id, hash,
    }));
    nextStamp++;
  });
}

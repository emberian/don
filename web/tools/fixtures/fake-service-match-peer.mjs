// Deterministic parser fixture for web/local-match.mjs. The real Chrome smoke points at
// the compiled Rust service-match-peer; this script only mutation-tests the bounded Node owner.

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
console.log(JSON.stringify({ event: 'turn', stamp: 0, packages: 2, hash: '0123456789abcdef' }));
console.log(JSON.stringify({ event: 'done', id: mode === 'host' ? 101 : 202, hash: '0123456789abcdef' }));

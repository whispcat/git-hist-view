// Builds the wasm crates into web/src/wasm. Usage: bun run wasm (or wasm:dev)
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

const dev = process.argv.includes('--dev');
const root = new URL('..', import.meta.url).pathname;
const target = 'wasm32-unknown-unknown';
const crates = ['ghv-wasm-core', 'ghv-wasm-parse'];

const run = (cmd, args, env = {}) => execFileSync(cmd, args, { cwd: root, stdio: ['ignore', 'pipe', 'inherit'], env: { ...process.env, ...env } }).toString();

const sysroot = run('rustc', ['--print', 'sysroot']).trim();
const metadata = JSON.parse(run('cargo', ['metadata', '--format-version', '1']));
const tsLanguage = metadata.packages.find((p) => p.name === 'tree-sitter-language');

// Apple's `ar` writes archives without a wasm symbol index, and published grammar crates
// don't yet add tree-sitter's wasm libc headers, so we supply both.
const env = {
  AR_wasm32_unknown_unknown: join(sysroot, 'lib/rustlib', run('rustc', ['-vV']).match(/host: (.*)/)[1], 'bin/llvm-ar'),
  CFLAGS_wasm32_unknown_unknown: `-I${join(tsLanguage.manifest_path, '..', 'wasm/include')}`,
};

run('cargo', ['build', '--release', '--target', target, ...crates.flatMap((c) => ['-p', c])], env);

for (const crate of crates) {
  const name = crate.replaceAll('-', '_');
  const out = join(root, 'web/src/wasm', crate.replace('ghv-wasm-', ''));
  run('wasm-bindgen', ['--target', 'web', '--out-dir', out, '--out-name', 'index', join(root, 'target', target, 'release', `${name}.wasm`)]);
  if (!dev) {
    const wasm = join(out, 'index_bg.wasm');
    run('wasm-opt', [
      '-O3',
      '--enable-simd',
      '--enable-bulk-memory',
      '--enable-nontrapping-float-to-int',
      '--enable-sign-ext',
      '--enable-reference-types',
      '--enable-multivalue',
      wasm,
      '-o',
      wasm,
    ]);
  }
  console.log(`built ${crate} -> ${out}`);
}

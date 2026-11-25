// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// The third-party licence notice (THIRD_PARTY_LICENSES.html, made by tools/build-licenses.sh): that it is there and says
// what it must, that every package takes it (tools/dist-common.sh) and says the game's licence and where its source is, that
// the page and the lobby link to it, and that tools/licenses/render.py refuses a licence that is not accepted. It does not
// run cargo-about: CI's `tools/build-licenses.sh --check` makes sure the file is current.
//
//   node --test tools/licenses.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, existsSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = file => readFileSync(join(root, file), 'utf8');
const page = read('THIRD_PARTY_LICENSES.html');
// The one place the project says where its source is (tools/dist-common.sh dist_source_url, src/menu.rs SOURCE_URL).
const repository = read('Cargo.toml').match(/^repository\s*=\s*"([^"]+)"/m)?.[1];

test('Cargo.toml names the repository, which the notice, the packages and the menu all read', () => {
  assert.match(repository ?? '', /^https:\/\/github\.com\/[^/]+\/hotdog-simulator$/);
  assert.ok(page.includes(`href="${repository}"`), 'the notice links to the source');
});

test('the licences page states the game licence and lists what ships that is not the project\'s', () => {
  assert.match(page, /^<!doctype html>/i);
  assert.match(page, /GNU General Public License, version 3 only/);
  assert.match(page, /without any warranty/);
  assert.ok(page.includes(read('LICENSE').split('\n').find(line => line.includes('GNU GENERAL PUBLIC LICENSE')).trim()), 'the full GPL');
  for (const needle of [
    'Fira Mono', 'Cantarell', 'Barlow Semi Condensed', 'Fredoka', 'SIL OPEN FONT LICENSE Version 1.1',
    'hotdog_bend.wgsl', 'hotdog_bend_prepass.wgsl', 'wgpu-hal', 'Pixabay Content License', 'freesound_community',
    'SMAA', 'Linearly Transformed Cosines', 'SDL_GameControllerDB',
    // Crates of every kind: the engine, the renderer, the sound, the web glue, the server, and a Windows-only and a wasm-only one.
    'bevy&nbsp;0.19.1', 'wgpu-hal&nbsp;29.0.4', 'wasm-bindgen&nbsp;0.2.128', 'tokio&nbsp;', 'axum&nbsp;', 'windows-sys&nbsp;', 'js-sys&nbsp;',
  ]) assert.ok(page.includes(needle), `the licences page names ${needle}`);
  assert.ok(!page.includes('hotdog-simulator&nbsp;0.1.0'), 'the project\'s own crates are not listed as third party');
  assert.ok(!/GPL-3\.0-only; \d+ crate/.test(page), 'no third-party crate is GPL-3.0-only');
  // Every crate is under a text that is printed (no entry without a licence).
  assert.ok((page.match(/<article class="lic"/g) ?? []).length > 100);
  assert.ok(Number((page.match(/\((\d+) of them\)/) ?? [])[1]) > 400, 'the crates are counted');
});

test('every package gets the licences page and a licence and source statement, from tools/dist-common.sh', () => {
  const scratch = mkdtempSync(join(tmpdir(), 'hotdog-dist-'));
  try {
    for (const platform of ['linux', 'windows', 'mac', 'web']) {
      const dest = join(scratch, platform);
      mkdirSync(dest);
      const run = spawnSync('bash', ['-c', '. tools/dist-common.sh; dist_stage_docs "$1" "$2"', '_', dest, platform], { cwd: root, encoding: 'utf8', env: { ...process.env, HOTDOG_VERSION: '0.1.0+abcdef0', HOTDOG_SOURCE_URL: '' } });
      assert.equal(run.status, 0, `${platform}: ${run.stderr}`);
      for (const file of ['README.md', 'LICENSE', 'COPYRIGHT', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_LICENSES.html', 'HOW-TO-RUN.txt']) {
        assert.ok(existsSync(join(dest, file)), `${platform} has ${file}`);
      }
      assert.equal(readFileSync(join(dest, 'THIRD_PARTY_LICENSES.html'), 'utf8'), page, `${platform}: the page as it is committed`);
      const howto = readFileSync(join(dest, 'HOW-TO-RUN.txt'), 'utf8').replace(/\r\n/g, '\n');
      assert.match(howto, /LICENCE AND SOURCE/, platform);
      assert.match(howto, /General Public\s+License, version 3 only/, platform);
      assert.ok(howto.includes(`      ${repository}\n`), `${platform}: where the source is`);
      assert.match(howto, /0\.1\.0\+abcdef0/, platform);
      assert.match(howto, /THIRD_PARTY_LICENSES\.html/, platform);
      if (platform === 'windows') assert.ok(readFileSync(join(dest, 'HOW-TO-RUN.txt'), 'utf8').includes('\r\n'), 'Windows line ends');
    }
    // An address in the environment replaces Cargo.toml's, for a fork or a build in CI that knows its own.
    const fork = join(scratch, 'fork');
    mkdirSync(fork);
    const run = spawnSync('bash', ['-c', '. tools/dist-common.sh; dist_stage_docs "$1" linux', '_', fork], { cwd: root, encoding: 'utf8', env: { ...process.env, HOTDOG_SOURCE_URL: 'https://example.org/fork/hotdog' } });
    assert.equal(run.status, 0, run.stderr);
    assert.ok(readFileSync(join(fork, 'HOW-TO-RUN.txt'), 'utf8').includes('      https://example.org/fork/hotdog\n'));
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('the Mac app carries the page in Contents/Resources, beside its assets, where the game\'s menu says it is', () => {
  assert.match(read('tools/build-mac.sh'), /cp "\$stage\/README\.md"[^\n]*"\$stage\/THIRD_PARTY_LICENSES\.html" "\$app\/Contents\/Resources\/"/);
  // The web package takes the documents into its root, which the server serves the page from.
  assert.match(read('tools/build-all.sh'), /dist_stage_docs "\$stage" web/);
  assert.match(read('crates/server/src/statics.rs'), /\^\/THIRD_PARTY_LICENSES\\\.html\$/);
});

test('the browser page and the lobby link to the licences page, and the menu opens it', () => {
  assert.match(read('index.html'), /<a class="legal" href="THIRD_PARTY_LICENSES\.html"/);
  assert.match(read('lobby.js'), /<a href="THIRD_PARTY_LICENSES\.html"/);
  assert.match(read('src/menu.rs'), /window\.open\('THIRD_PARTY_LICENSES\.html'/);
});

// render.py on a made-up cargo-about result: a text printed once for the crates it covers, the project's own crates left out,
// and a licence that is not accepted (or a third party's GPL-3.0-only) refused.
const crate = (name, version, license, extra = {}) => ({
  name, version, license, authors: ['Ada Lovelace <ada@example.org>'], id: `registry+https://github.com/rust-lang/crates.io-index#${name}@${version}`,
  source: 'registry+https://github.com/rust-lang/crates.io-index', manifest_path: `${root}/.cargo/registry/src/x/${name}-${version}/Cargo.toml`, ...extra,
});
const mit = 'Copyright (c) 2020 Someone\n\nPermission is hereby granted, free of charge, a-made-up-text-printed-once.';
function render(licenses) {
  const scratch = mkdtempSync(join(tmpdir(), 'hotdog-render-'));
  try {
    writeFileSync(join(scratch, 'found.json'), JSON.stringify({ overview: [], crates: [], licenses }));
    const run = spawnSync('python3', ['-I', 'tools/licenses/render.py', '--config', 'about.toml', '--gpl', 'LICENSE', '--copyright', 'COPYRIGHT', '--other', 'tools/licenses/other.json',
      '--source-url', 'https://example.org/src', '-o', join(scratch, 'out.html'), join(scratch, 'found.json')], { cwd: root, encoding: 'utf8' });
    return { run, out: existsSync(join(scratch, 'out.html')) ? readFileSync(join(scratch, 'out.html'), 'utf8') : '' };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}
const entry = (id, name, text, crates, source_path = '/x/LICENSE') => ({ id, name, text, source_path, used_by: crates.map(c => ({ crate: c, path: '' })) });
// The crates other.json points at ({crate:bevy_text}, ...) have to be among them.
const needed = ['bevy_text', 'sctk-adwaita', 'bevy_pbr', 'bevy_core_pipeline', 'bevy_anti_alias', 'gilrs', 'wgpu-hal'].map(n => crate(n, '1.0.0', 'MIT'));

test('render.py prints a licence text once for every crate it covers, and not the project\'s own crates', () => {
  const own = crate('hotdog-simulator', '0.1.0', 'GPL-3.0-only', { source: null, manifest_path: `${root}/Cargo.toml` });
  const { run, out } = render([entry('MIT', 'MIT License', mit, [...needed, crate('alpha', '1.2.3', 'MIT'), crate('beta', '0.4.0', 'MIT OR Apache-2.0')]), entry('GPL-3.0-only', 'GPL', 'the GPL', [own])]);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(out.split('a-made-up-text-printed-once').length, 2, 'one copy of the text');
  assert.ok(out.includes('alpha&nbsp;1.2.3') && out.includes('beta&nbsp;0.4.0'));
  assert.ok(!out.includes('hotdog-simulator&nbsp;0.1.0'));
  assert.ok(out.includes('href="https://example.org/src"'));
  assert.ok(out.includes('Copyright (C) 2026'), 'the project\'s Copyright line is COPYRIGHT\'s');
  // A text that names no holder has the crate's authors beside it, without their addresses.
  const bare = render([entry('MIT', 'MIT License', 'Permission is hereby granted ...', [...needed, crate('gamma', '2.0.0', 'MIT')])]);
  assert.ok(bare.out.includes('(Ada Lovelace)') && !bare.out.includes('ada@example.org'));
});

test('render.py refuses a licence that about.toml does not accept, and a third party\'s GPL-3.0-only', () => {
  const unaccepted = render([entry('AGPL-3.0-only', 'AGPL', 'text', [...needed, crate('delta', '1.0.0', 'AGPL-3.0-only')])]);
  assert.equal(unaccepted.run.status, 1);
  assert.match(unaccepted.run.stderr, /delta 1\.0\.0: AGPL-3\.0-only is not in about\.toml's accepted list/);
  const copyleft = render([entry('GPL-3.0-only', 'GPL', 'text', [...needed, crate('epsilon', '1.0.0', 'GPL-3.0-only')])]);
  assert.equal(copyleft.run.status, 1);
  assert.match(copyleft.run.stderr, /epsilon 1\.0\.0 is GPL-3\.0-only and is not the project's own/);
});

test('about.toml accepts only permissive licences (and GPL-3.0-only, for the project\'s own crates)', () => {
  const accepted = [...read('about.toml').match(/accepted = \[([^\]]*)\]/)[1].matchAll(/^\s*"([^"]+)"/gm)].map(m => m[1]);
  const permissive = new Set(['MIT', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'BSL-1.0', 'Unicode-3.0', 'Unlicense', 'CC0-1.0', 'MIT-0', '0BSD', 'CDLA-Permissive-2.0', 'GPL-3.0-only']);
  for (const id of accepted) assert.ok(permissive.has(id), `${id} is not on the list of licences known to be compatible with GPL-3.0-only: read it, then add it here`);
  assert.ok(accepted.includes('MIT') && accepted.includes('Apache-2.0'));
});

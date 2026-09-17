import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';

const extensionRoot = resolve(import.meta.dirname, '..');
const workspaceRoot = resolve(extensionRoot, '..', '..');
const generatedFile = resolve(extensionRoot, 'src/generated/frontal-code-events.ts');
const tempDir = mkdtempSync(join(tmpdir(), 'frontal-code-events-check-'));
const tempFile = join(tempDir, 'frontal-code-events.ts');

try {
  execFileSync(
    'cargo',
    [
      'run',
      '-p',
      'frontal-code-events',
      '--bin',
      'export-typescript',
      '--',
      tempFile,
    ],
    {
      cwd: workspaceRoot,
      stdio: 'pipe',
    }
  );

  const expected = readFileSync(tempFile, 'utf8');
  const actual = readFileSync(generatedFile, 'utf8');
  if (actual !== expected) {
    process.stderr.write(
      [
        'Generated Frontal Code event bindings are stale.',
        'Run `npm run sync:frontal-code-events` in extensions/frontal-code-slack and commit the updated file.',
        '',
      ].join('\n')
    );
    process.exit(1);
  }
} finally {
  rmSync(tempDir, { recursive: true, force: true });
}

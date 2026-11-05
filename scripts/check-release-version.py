"""Keep Rust, JS, Tauri and the release tag in sync before signing."""
import json
import os
import pathlib
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent
package = json.loads((root / 'package.json').read_text())['version']
lock = json.loads((root / 'package-lock.json').read_text())
config = json.loads((root / 'src-tauri/tauri.conf.json').read_text())
rust = tomllib.loads((root / 'Cargo.toml').read_text())
assert package == lock['version'] == lock['packages']['']['version']
assert package == config['version'] == rust['workspace']['package']['version']
assert config['bundle']['createUpdaterArtifacts'] is True
assert config['plugins']['updater']['pubkey'] != 'PENDING_SIGNING_KEY'
assert config['plugins']['updater']['endpoints'] == [
    'https://github.com/matsushibadenki/LumaPaint/releases/latest/download/latest.json'
]
ref = os.environ.get('GITHUB_REF', '')
if ref.startswith('refs/tags/'):
    assert ref == f'refs/tags/v{package}', (ref, package)
print(f'Release versions and update configuration agree: {package}')

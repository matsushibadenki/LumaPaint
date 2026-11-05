"""Reject incomplete release manifests before publishing the draft."""
import base64
import json
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parent.parent
version = json.loads((root / 'package.json').read_text())['version']
manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert manifest['version'].removeprefix('v') == version
platform = manifest['platforms']['darwin-aarch64']
assert platform['url'].startswith(
    f'https://github.com/matsushibadenki/LumaPaint/releases/download/v{version}/'
)
assert platform['url'].endswith('.tar.gz')
assert base64.b64decode(platform['signature'], validate=True).startswith(b'untrusted comment:')
print(f'Updater manifest is complete for macOS Apple Silicon: {version}')

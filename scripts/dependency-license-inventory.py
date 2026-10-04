#!/usr/bin/env python3
"""Generate the locked Cargo dependency inventory, including source license files.
This inventories metadata; it does not replace license or distribution review.
"""
import argparse
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=pathlib.Path, required=True)
args = parser.parse_args()
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--locked', '--offline', '--format-version', '1'], cwd=root))
rows = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if package['id'] in metadata['workspace_members']:
        continue
    folder = pathlib.Path(package['manifest_path']).parent
    notices = sorted(p.name for p in folder.iterdir()
                     if p.is_file() and p.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE')))
    rows.append({'name': package['name'], 'version': package['version'],
                 'license': package['license'], 'license_file': package['license_file'],
                 'source_notices': notices, 'source': package['source'],
                 'repository': package['repository']})
result = {'scope': 'Cargo.lock packages across targets and features; may include unlinked packages',
          'native_notices': 'third-party-notices/README.md', 'dependencies': rows}
args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
print(f'{len(rows)} dependencies inventoried; '
      f'{sum(not r["license"] and not r["license_file"] for r in rows)} lack license metadata.')

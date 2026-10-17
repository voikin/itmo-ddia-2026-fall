"""Fill only placeholders in the local .env; preserve existing secrets; print none."""
import os
import re
import secrets
from pathlib import Path

root = Path(__file__).resolve().parents[1]
example, env = root / '.env.example', root / '.env'
if not env.exists():
    raise SystemExit('First run: cp .env.example .env')


def parse(path):
    values = {}
    for line in path.read_text().splitlines():
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        key, sep, value = line.partition('=')
        if not sep or key in values:
            raise SystemExit(f'{path.name}: malformed or duplicate entry')
        values[key] = value
    return values


expected, values = parse(example), parse(env)
unknown = set(values) - set(expected)
if unknown:
    raise SystemExit(f'Unexpected keys in .env: {sorted(unknown)}')
for key, default in expected.items():
    values.setdefault(key, default)

for key in ('GRAFANA_ADMIN_PASSWORD',):
    if values[key] == 'GENERATE_LOCALLY':
        values[key] = secrets.token_hex(32)
    if not re.fullmatch(r'[0-9a-f]{32,128}', values[key]):
        raise SystemExit(f'{key}: expected 32..128 lowercase hex characters; value not printed')

for key in ('APP_POD', 'APP_DATACENTER', 'APP_DEPLOYMENT'):
    if not re.fullmatch(r'[a-zA-Z0-9_.-]{1,63}', values[key]):
        raise SystemExit(f'{key}: expected 1..63 of [a-zA-Z0-9_.-]')
if not values['APP_PORT'].isdigit() or not 1 <= int(values['APP_PORT']) <= 65535:
    raise SystemExit('APP_PORT: expected integer 1..65535')

os.chmod(env, 0o600)
env.write_text(''.join(f'{k}={v}\n' for k, v in values.items()))
print('.env configured (mode 0600); secret values were not printed.')

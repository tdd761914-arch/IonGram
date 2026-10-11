#!/usr/bin/env python3
"""Fetch exact third-party API definitions. Never silently follow upstream main."""
import hashlib,json,pathlib,urllib.request
root=pathlib.Path(__file__).resolve().parents[1]
for name,source in json.loads((root/'schema/sources.json').read_text()).items():
    path=root/'schema'/name
    if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest()==source['sha256']:
        print(f'{name}: verified');continue
    with urllib.request.urlopen(source['url'],timeout=60) as response:data=response.read()
    if hashlib.sha256(data).hexdigest()!=source['sha256']:raise SystemExit(f'{name}: checksum mismatch')
    path.write_bytes(data)
    print(f'{name}: downloaded and verified')

#!/usr/bin/env python3
"""실제 CPU CLI·HTTP 실행, 경로 설정, 실패 전파와 테넌트 격리를 검증한다."""

import argparse
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=root / 'target/debug')
    parser.add_argument('--output-dir', type=Path, default=root / 'target/qa-smoke')
    parser.add_argument('--cli-path-mode', choices=['override', 'sibling'], default='override')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    binaries = args.bin_dir.resolve()
    work = Path(tempfile.mkdtemp(prefix='live-smoke-', dir=output))
    env = dict(os.environ, XAZZ_BACKEND='cpu')
    env.pop('XAZZ_EXEC_PATH', None)

    def cli(*arguments, cwd=work):
        result = subprocess.run(
            [str(binaries / 'xazz'), *arguments], cwd=cwd, env=env,
            text=True, capture_output=True, timeout=90,
        )
        print(json.dumps({
            'command': arguments, 'exit': result.returncode,
            'stdout': result.stdout, 'stderr': result.stderr,
        }, ensure_ascii=False))
        assert result.returncode == 0, result.stderr
        return result

    cli('new', 'sample-project')
    project = work / 'sample-project'
    cli('import', 'data/sample.csv', cwd=project)
    cli('check', 'main.xzz', '--json', cwd=project)
    result = json.loads(cli('run', 'main.xzz', '--json', cwd=project).stdout)
    assert result['success'] and len(result['rows']) == 10, result
    cli('run', 'example.xzz', '--json', cwd=project)

    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    env.update(
        TMPDIR=str(project), XAZZ_BIND=f'127.0.0.1:{port}',
        XAZZ_WEB_DIR=str(root / 'visual-ide/dist'),
        XAZZ_TENANT_TOKENS='qa-a=local-a,qa-b=local-b',
    )

    if args.cli_path_mode == 'override':
        env['XAZZ_EXEC_PATH'] = str(binaries / 'xazz')

    def request(path, body=None, tenant='qa-a'):
        token = 'local-a' if tenant == 'qa-a' else 'local-b'
        req = urllib.request.Request(
            f'http://127.0.0.1:{port}{path}',
            data=None if body is None else json.dumps(body).encode(),
            headers={
                'Authorization': f'Bearer {token}',
                'X-Xazz-Tenant': tenant, 'Content-Type': 'application/json',
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=90) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode()

    with (output / 'http-server.log').open('w') as log:
        server = subprocess.Popen(
            [str(binaries / 'xazz-server')], cwd=project, env=env,
            stdout=log, stderr=log,
        )
        try:
            for _ in range(100):
                try:
                    if request('/health')[0] == 200:
                        break
                except OSError:
                    time.sleep(0.1)
            else:
                raise AssertionError('server did not start')

            status, result = request('/execute', {'code': (project / 'main.xzz').read_text()})
            print(json.dumps({'execute_status': status, 'execute': result}, ensure_ascii=False))
            assert status == 200 and result['success'] and len(result['rows']) == 10, result
            run_id = result['run_id']
            status, resources = request(f'/runs/{run_id}/resources')
            print(json.dumps({'resources_status': status, 'resources': resources}))
            assert status == 200 and resources['available'], resources
            counters = resources['resources']
            assert counters['source'] == 'runner-process-tree', counters
            assert all(counters[key] >= 0 for key in ('duration_ms', 'cpu_user_ms', 'cpu_sys_ms'))
            assert counters['max_rss_kb'] > 0, counters
            status, _ = request(f'/runs/{run_id}/resources', tenant='qa-b')
            assert status == 404, status

            # Only this smoke run's temporary database is changed.
            with sqlite3.connect(project / 'xazz.db') as database:
                database.execute('UPDATE runs SET resources=NULL WHERE id=?', (run_id,))
            status, unavailable = request(f'/runs/{run_id}/resources')
            print(json.dumps({'legacy_resources': unavailable}))
            assert status == 200 and unavailable['available'] is False, unavailable
            failure_code = 'type S = { x: float }; v broken = load("missing.csv") :: S;'
            status, failure = request('/execute', {'code': failure_code})
            print(json.dumps({'runtime_failure': failure}, ensure_ascii=False))
            assert status == 200 and failure['success'] is False, failure
            (project / 'failure.xzz').write_text(failure_code)
            failed = subprocess.run(
                [str(binaries / 'xazz'), 'run', 'failure.xzz', '--json'],
                cwd=project, env={key: value for key, value in env.items() if key != 'XAZZ_EXEC_PATH'},
                text=True, capture_output=True, timeout=90,
            )
            failed_json = json.loads(failed.stdout)
            assert failed.returncode == 1 and failed_json['success'] is False, failed_json
            assert failed_json['exit_code'] == 1, failed_json
            print(json.dumps({'cli_path_mode': args.cli_path_mode, 'failure_propagation': 'passed'}))
            print('SMOKE PASSED: new/import/check/run; HTTP execute/resources; '
                  'cross-tenant 404; legacy unavailable; failure propagation')
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=10)


if __name__ == '__main__':
    main()

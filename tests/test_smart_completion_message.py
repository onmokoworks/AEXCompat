"""Actual selector diagnostic must survive failed Smart output validation."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize('mode', ['message', 'silent', 'long', 'private'])
def test_smart_completion_keeps_selector_diagnostic(tmp_path, mode):
    plugin = os.environ.get('AEXCOMPAT_TEST_SMART_MESSAGE_PROBE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_SMART_MESSAGE_PROBE to compiled map-layer probe')
    source, output, request = (tmp_path / n for n in ('input.png', 'output.png', 'request.json'))
    Image.new('RGBA', (16, 12), (30, 80, 140, 255)).save(source)
    request.write_text(json.dumps({'schema_version': 1, 'assignments': []}), encoding='utf-8')
    env = dict(os.environ, AEXCOMPAT_PROBE_RETURN_MESSAGE=mode)
    run = subprocess.run([str(ROOT / 'broker/target/release/aexcompat-harness.exe'),
        '--headless', '--render-experimental-smart-request', plugin,
        str(source), str(output), str(request)], cwd=ROOT, env=env,
        capture_output=True, timeout=60)
    (tmp_path / 'stdout.json').write_bytes(run.stdout)
    (tmp_path / 'stderr.log').write_bytes(run.stderr)
    assert run.returncode != 0 and not output.exists()
    error = run.stderr.decode('utf-8')
    assert 'malformed_error_response' not in error
    report, _ = json.JSONDecoder().raw_decode(error.split('report=', 1)[1])
    assert report['smart_render_selector_error'] == 0
    assert report['output_pixels_valid'] is False
    if mode == 'message':
        assert report['return_message'] == {
            'selector': 'SMART_RENDER', 'text': 'synthetic Smart prerequisite missing',
            'error': 0, 'display_requested': True}
    elif mode == 'long':
        assert report['return_message'] == {
            'selector': 'SMART_RENDER', 'text': 'x' * 255,
            'error': 0, 'display_requested': True}
    else:
        assert report.get('return_message') is None
        assert 'private-probe' not in error

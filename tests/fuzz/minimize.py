"""Line-based delta minimizer: keeps removing lines (and balanced blocks)
while the program still compiles and still fails verification."""
import os
import subprocess
import sys
import tempfile

COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
source = open(sys.argv[1], encoding='utf-8').read().splitlines()
needle = sys.argv[2] if len(sys.argv) > 2 else 'VerifyError'
work = tempfile.mkdtemp()


def bad(lines):
    path = os.path.join(work, 'm.mam')
    open(path, 'w', encoding='utf-8').write('\n'.join(lines) + '\n')
    out = os.path.join(work, 'o')
    subprocess.run(['cmd', '/c', 'rmdir', '/s', '/q', out], capture_output=True)
    if subprocess.run([COMPILER, path, out], capture_output=True).returncode != 0:
        return False
    try:
        ran = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=4)
    except subprocess.TimeoutExpired:
        return False
    return needle in ran.stderr.decode('utf-8', 'replace')


assert bad(source), 'original does not fail'
changed = True
while changed:
    changed = False
    size = len(source)
    chunk = max(1, size // 2)
    while chunk >= 1:
        index = 0
        while index < len(source):
            candidate = source[:index] + source[index + chunk:]
            if candidate and bad(candidate):
                source = candidate
                changed = True
            else:
                index += chunk
        chunk //= 2
print('\n'.join(source))

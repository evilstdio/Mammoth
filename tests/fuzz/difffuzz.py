"""Differential fuzzer: every generated program exists in Mammoth and in Java.
Both run under -Xverify:all; stdout and crash status must match."""
import os
import random
import re
import subprocess
import sys
import tempfile

COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
BOX = {'I': 'Integer', 'L': 'Long', 'D': 'Double', 'B': 'Boolean', 'T': 'String', 'NT': 'String', 'NI': 'Integer'}
PRIM = {'I': 'int', 'L': 'long', 'D': 'double', 'B': 'boolean', 'T': 'String', 'NT': 'String', 'NI': 'Integer'}
MAM = {'I': 'Int', 'L': 'Long', 'D': 'Double', 'B': 'Bool', 'T': 'Text', 'NT': 'Text?', 'NI': 'Int?'}
RANK = {'I': 0, 'L': 1, 'D': 2}


class Gen:
    def __init__(self, rng):
        self.r = rng
        self.m = []
        self.j = []
        self.funcs = []
        self.n = 0
        self.vars = {t: [] for t in MAM}       # (mammoth name, java read expr, mutable)
        self.lists = []                         # (name, length)
        self.indent = 0
        self.in_loop = False
        self.in_function = None
        self.in_lambda = False

    def fresh(self, p):
        self.n += 1
        return f'{p}{self.n}'

    def emit(self, m, j):
        pad = '    ' * self.indent
        if m is not None:
            self.m.append(pad + m)
        if j is not None:
            self.j.append(pad + j)

    # ---- expressions: return (mammoth, java) ----
    def pick(self, t):
        return self.r.choice(self.vars[t]) if self.vars[t] else None

    def literal(self, t):
        r = self.r
        if t == 'I':
            v = r.choice([0, 1, 2, -1, 3, 7, -5, 100, 2147483647, -2147483647, r.randint(-9, 9)])
            return (str(v) if v >= 0 else f'({v})', str(v) if v >= 0 else f'({v})')
        if t == 'L':
            v = r.choice([0, 1, -1, 5000000000, -5000000000, 9223372036854775807, r.randint(-9, 9)])
            return (f'{v}L' if v >= 0 else f'({v}L)', f'{v}L' if v >= 0 else f'({v}L)')
        if t == 'D':
            v = r.choice(['0.0', '1.0', '2.5', '0.1', '1000000.0', '100000000.0', '3.0'])
            neg = r.random() < 0.3
            return (f'(-{v})' if neg else v, f'(-{v})' if neg else v)
        if t == 'B':
            v = r.choice(['true', 'false'])
            return (v, v)
        if t == 'T':
            v = r.choice(['', 'a', 'hello', 'x y'])
            return (f'"{v}"', f'"{v}"')
        return ('null', '((%s) null)' % BOX[t])

    def traced(self, t):
        if t in ('I', 'L', 'D', 'B', 'T'):
            am, aj = self.expr(t, 3)
            return (f'tr{t}({am})', f'tr{t}({aj})')
        return self.literal(t)

    def expr(self, t, depth=0, plain=False):
        r = self.r
        choices = ['lit', 'var', 'var', 'trace', 'if', 'when', 'whens', 'op', 'op', 'call', 'lambda']
        if depth >= 3 or plain:
            choices = ['lit', 'var', 'var'] if plain else ['lit', 'var', 'trace']
        kind = r.choice(choices)
        if kind == 'var':
            v = self.pick(t)
            if v:
                return (v[0], v[1])
            if t in ('L', 'D') and self.vars['I'] and r.random() < 0.3 and False:
                pass
            kind = 'lit'
        if kind == 'lit':
            return self.literal(t)
        if kind == 'trace':
            return self.traced(t)
        if kind == 'if':
            c = self.expr('B', depth + 1)
            a = self.expr(t, depth + 1)
            b = self.expr(t, depth + 1)
            return (f'if {c[0]} {{ {a[0]} }} else {{ {b[0]} }}', f'({c[1]} ? {a[1]} : {b[1]})')
        if kind == 'when':
            return self.when_value(t, depth)
        if kind == 'whens':
            return self.when_subjectless(t, depth)
        if kind == 'call':
            options = [f for f in self.funcs if f[2] == t]
            if options:
                f = r.choice(options)
                args = [self.expr(p, depth + 1) for p in f[1]]
                m = ', '.join(a[0] for a in args)
                j = ', '.join(a[1] for a in args)
                return (f'{f[0]}({m})', f'{f[0]}({j})')
            return self.literal(t)
        if kind == 'lambda' and t in ('I', 'L', 'D'):
            return self.literal(t)
        if kind == 'op':
            return self.operator(t, depth)
        return self.literal(t)

    def operator(self, t, depth):
        r = self.r
        if t in ('I', 'L', 'D'):
            lower = [x for x in ('I', 'L', 'D') if RANK[x] <= RANK[t]]
            lt = r.choice(lower)
            rt = t if RANK[lt] < RANK[t] else r.choice(lower)
            a = self.expr(lt, depth + 1)
            b = self.expr(rt, depth + 1)
            op = r.choice(['+', '-', '*', '/', '%'])
            return (f'({a[0]} {op} {b[0]})', f'({a[1]} {op} {b[1]})')
        if t == 'B':
            k = r.choice(['cmp', 'cmp', 'not', 'and', 'or', 'teq', 'beq', 'nullcheck'])
            if k == 'cmp':
                lt = r.choice(['I', 'L', 'D'])
                rt = r.choice(['I', 'L', 'D'])
                a = self.expr(lt, depth + 1)
                b = self.expr(rt, depth + 1)
                op = r.choice(['<', '<=', '>', '>=', '==', '!='])
                return (f'({a[0]} {op} {b[0]})', f'({a[1]} {op} {b[1]})')
            if k == 'not':
                a = self.expr('B', depth + 1)
                return (f'(not {a[0]})', f'(!{a[1]})')
            if k in ('and', 'or'):
                a = self.expr('B', depth + 1)
                b = self.expr('B', depth + 1)
                return (f'({a[0]} {k} {b[0]})', f'({a[1]} {"&&" if k == "and" else "||"} {b[1]})')
            if k == 'teq':
                a = self.expr('T', depth + 1)
                b = self.expr('T', depth + 1)
                op = r.choice(['==', '!='])
                j = f'java.util.Objects.equals({a[1]}, {b[1]})'
                return (f'({a[0]} {op} {b[0]})', j if op == '==' else f'(!{j})')
            if k == 'beq':
                a = self.expr('B', depth + 1)
                b = self.expr('B', depth + 1)
                op = r.choice(['==', '!='])
                return (f'({a[0]} {op} {b[0]})', f'({a[1]} {op} {b[1]})')
            v = self.pick('NT')
            if v:
                op = r.choice(['==', '!='])
                return (f'({v[0]} {op} null)', f'({v[1]} {op} null)')
            return self.literal('B')
        if t == 'T':
            k = r.choice(['concat', 'template', 'upper', 'elvis'])
            if k == 'concat':
                a = self.expr('T', depth + 1)
                b = self.expr(r.choice(['T', 'I', 'L', 'D', 'B']), depth + 1)
                return (f'({a[0]} + {b[0]})', f'({a[1]} + {b[1]})')
            if k == 'template':
                a = self.expr(r.choice(['I', 'L', 'D', 'B', 'T']), 3, plain=True)
                return (f'"<{{{a[0]}}}>"', f'("<" + {a[1]} + ">")')
            if k == 'upper':
                a = self.expr('T', depth + 1)
                return (f'{a[0]}.toUpperCase()', f'{a[1]}.toUpperCase()')
            v = self.pick('NT')
            if v:
                b = self.expr('T', depth + 1)
                return (f'({v[0]} ?: {b[0]})', f'({v[1]} != null ? {v[1]} : {b[1]})')
            return self.literal('T')
        if t == 'NT':
            a = self.expr('T', depth + 1)
            return a
        if t == 'NI':
            v = self.pick('NT')
            if v:
                return (f'{v[0]}?.length()', f'({v[1]} != null ? (Integer) {v[1]}.length() : null)')
            a = self.expr('I', depth + 1)
            return (a[0], f'((Integer) {a[1]})')
        return self.literal(t)

    def wrap(self, t, body):
        cast = '' if t in ('T', 'NT', 'NI') else f'({PRIM[t]}) '
        return f'({cast}((java.util.function.Supplier<{BOX[t]}>) () -> {{ {body} }}).get())'

    def when_value(self, t, depth):
        r = self.r
        pad = '    ' * (self.indent + 1)
        s = self.expr('I', depth + 1)
        tmp = self.fresh('w')
        m = [f'when {s[0]} {{']
        j = [f'int {tmp} = {s[1]};']
        for _ in range(r.randint(1, 3)):
            v = self.expr(t, depth + 1)
            kind = r.choice(['one', 'many', 'range'])
            if kind == 'one':
                c = r.randint(-2, 4)
                m.append(f'{c} -> {v[0]}')
                j.append(f'if ({tmp} == {c}) return {v[1]};')
            elif kind == 'many':
                c, d = r.randint(-2, 4), r.randint(-2, 4)
                m.append(f'{c}, {d} -> {v[0]}')
                j.append(f'if ({tmp} == {c} || {tmp} == {d}) return {v[1]};')
            else:
                c, d = sorted((r.randint(-2, 4), r.randint(-2, 4)))
                m.append(f'in {c}..{d} -> {v[0]}')
                j.append(f'if ({tmp} >= {c} && {tmp} <= {d}) return {v[1]};')
        v = self.expr(t, depth + 1)
        m.append(f'else -> {v[0]} }}')
        j.append(f'return {v[1]};')
        return ('\n'.join((pad if i else '') + x for i, x in enumerate(m)), self.wrap(t, ' '.join(j)))

    def when_subjectless(self, t, depth):
        r = self.r
        pad = '    ' * (self.indent + 1)
        m = ['when {']
        j = []
        for _ in range(r.randint(1, 2)):
            c = self.expr('B', depth + 1)
            v = self.expr(t, depth + 1)
            m.append(f'{c[0]} -> {v[0]}')
            j.append(f'if ({c[1]}) return {v[1]};')
        v = self.expr(t, depth + 1)
        m.append(f'else -> {v[0]} }}')
        j.append(f'return {v[1]};')
        return ('\n'.join((pad if i else '') + x for i, x in enumerate(m)), self.wrap(t, ' '.join(j)))

    # ---- statements ----
    def declare(self, mutable=None):
        r = self.r
        t = r.choice(['I', 'I', 'L', 'D', 'B', 'T', 'NT', 'NI'])
        mutable = r.random() < 0.6 if mutable is None else mutable
        name = self.fresh('v')
        e = self.expr(t, 0)
        kw = 'var' if mutable else 'let'
        anno = f': {MAM[t]}'
        self.emit(f'{kw} {name}{anno} = {e[0]}', None)
        if mutable:
            self.emit(None, f'final {PRIM[t]}[] {name} = new {PRIM[t]}[1]; {name}[0] = {e[1]};')
            read = f'{name}[0]'
        else:
            self.emit(None, f'final {PRIM[t]} {name} = {e[1]};')
            read = name
        # A mutable nullable is never narrowed; both languages see the same type.
        self.vars[t].append((name, read, mutable))

    def assign(self):
        r = self.r
        pool = [(t, v) for t in self.vars for v in self.vars[t] if v[2] and t in ('I', 'L', 'D', 'T', 'NT', 'NI', 'B')]
        if not pool:
            return self.declare(True)
        t, v = r.choice(pool)
        if t in ('I', 'L', 'D') and r.random() < 0.6:
            op = r.choice(['+=', '-=', '*='])
            e = self.expr('I' if t == 'I' else r.choice([x for x in ('I', 'L', 'D') if RANK[x] <= RANK[t]]))
            self.emit(f'{v[0]} {op} {e[0]}', f'{v[1]} {op} {e[1]};')
            return
        e = self.expr(t)
        self.emit(f'{v[0]} = {e[0]}', f'{v[1]} = {e[1]};')

    def show(self):
        t = self.r.choice(['I', 'L', 'D', 'B', 'T', 'NT', 'NI'])
        if t in ('NT', 'NI'):
            # An untyped null branch needs a declared type in Mammoth.
            v = self.pick(t)
            if not v:
                return self.declare()
            e = (v[0], v[1])
        else:
            e = self.expr(t)
        self.emit(f'print({e[0]})', f'System.out.println({e[1]});')

    def list_decl(self):
        r = self.r
        n = r.randint(1, 3)
        items = [self.expr('I', 1) for _ in range(n)]
        name = self.fresh('xs')
        self.emit(f'let {name} = [{", ".join(i[0] for i in items)}]', f'final int[] {name} = {{{", ".join(i[1] for i in items)}}};')
        self.lists.append((name, n))

    def list_ops(self):
        r = self.r
        if not self.lists:
            return self.list_decl()
        name, n = r.choice(self.lists)
        i = r.randint(0, n - 1) if r.random() < 0.9 else n
        idx = (f'trI({i})', f'trI({i})') if r.random() < 0.5 else (str(i), str(i))
        k = r.choice(['print', 'set', 'compound'])
        if k == 'print':
            self.emit(f'print({name}[{idx[0]}])', f'System.out.println({name}[{idx[1]}]);')
        elif k == 'set':
            e = self.expr('I', 1)
            self.emit(f'{name}[{idx[0]}] = {e[0]}', f'{name}[{idx[1]}] = {e[1]};')
        else:
            e = self.expr('I', 1)
            op = r.choice(['+=', '-='])
            self.emit(f'{name}[{idx[0]}] {op} {e[0]}', f'{name}[{idx[1]}] {op} {e[1]};')

    def closure(self):
        r = self.r
        pool = self.vars['I']
        name = self.fresh('f')
        p = self.fresh('p')
        saved = {t: list(v) for t, v in self.vars.items()}
        self.vars['I'] = self.vars['I'] + [(p, p, False)]
        mutable = [v for v in pool if v[2]]
        was = self.in_lambda
        self.in_lambda = True
        result = self.expr('I', 1)
        self.in_lambda = was
        self.vars = saved
        if mutable and r.random() < 0.6:
            v = r.choice(mutable)
            self.emit(f'let {name} = {{ {p}: Int -> {v[0]} += 1', None)
            self.emit(f'    {result[0]} }}', None)
            self.emit(None, f'final java.util.function.IntUnaryOperator {name} = ({p}) -> {{ {v[1]} += 1; return {result[1]}; }};')
        else:
            self.emit(f'let {name} = {{ {p}: Int -> {result[0]} }}',
                      f'final java.util.function.IntUnaryOperator {name} = ({p}) -> {result[1]};')
        arg = self.expr('I', 2)
        self.emit(f'print({name}({arg[0]}))', f'System.out.println({name}.applyAsInt({arg[1]}));')

    def block(self, budget):
        saved = {t: list(v) for t, v in self.vars.items()}
        saved_lists = list(self.lists)
        self.indent += 1
        for _ in range(self.r.randint(1, 3)):
            self.statement(budget)
        self.indent -= 1
        self.vars = saved
        self.lists = saved_lists

    def statement(self, budget):
        r = self.r
        options = ['declare', 'declare', 'assign', 'assign', 'show', 'show', 'list', 'closure']
        if budget > 0:
            options += ['if', 'while', 'for', 'try', 'tryf']
        if self.in_loop:
            options += ['break', 'continue']
        if self.in_function:
            options += ['return']
        k = r.choice(options)
        if k == 'declare':
            self.declare()
        elif k == 'assign':
            self.assign()
        elif k == 'show':
            self.show()
        elif k == 'list':
            self.list_ops()
        elif k == 'closure':
            self.closure()
        elif k == 'if':
            c = self.expr('B', 1)
            self.emit(f'if {c[0]} {{', f'if ({c[1]}) {{')
            self.block(budget - 1)
            if r.random() < 0.5:
                self.emit('} else {', '} else {')
                self.block(budget - 1)
            self.emit('}', '}')
        elif k == 'while':
            i = self.fresh('i')
            limit = r.randint(1, 3)
            self.emit(f'var {i} = 0', f'int {i} = 0;')
            self.emit(f'while {i} < {limit} {{', f'while ({i} < {limit}) {{')
            self.emit(f'    {i} += 1', f'    {i} += 1;')
            was = self.in_loop
            self.in_loop = True
            self.block(budget - 1)
            self.in_loop = was
            self.emit('}', '}')
        elif k == 'for':
            i = self.fresh('i')
            a, b = r.randint(-1, 1), r.randint(0, 3)
            self.emit(f'for {i} in {a}..{b} {{', f'for (int {i}_ = {a}; {i}_ <= {b}; {i}_++) {{ final int {i} = {i}_;')
            was = self.in_loop
            self.in_loop = True
            saved = list(self.vars['I'])
            self.vars['I'].append((i, i, False))
            self.block(budget - 1)
            self.vars['I'] = saved
            self.in_loop = was
            self.emit('}', '}')
        elif k in ('try', 'tryf'):
            self.emit('try {', 'try {')
            if r.random() < 0.6:
                c = self.expr('B', 2)
                self.emit(f'    if {c[0]} {{ throw IllegalStateException("x") }}',
                          f'    if ({c[1]}) {{ throw new IllegalStateException("x"); }}')
            self.block(budget - 1)
            finally_only = k == 'tryf' and r.random() < 0.3
            if not finally_only:
                name = self.fresh('e')
                cls = r.choice(['IllegalStateException', 'ArithmeticException', 'RuntimeException'])
                self.emit(f'}} catch ({name}: {cls}) {{', f'}} catch ({cls} {name}) {{')
                self.block(budget - 1)
            if k == 'tryf':
                self.emit('} finally {', '} finally {')
                was, self.in_loop = self.in_loop, False
                saved = self.in_function
                self.in_function = None
                self.block(budget - 2)
                self.in_function = saved
                self.in_loop = was
            self.emit('}', '}')
        elif k == 'break':
            c = self.expr('B', 2)
            self.emit(f'if {c[0]} {{ break }}', f'if ({c[1]}) {{ break; }}')
        elif k == 'continue':
            c = self.expr('B', 2)
            self.emit(f'if {c[0]} {{ continue }}', f'if ({c[1]}) {{ continue; }}')
        elif k == 'return':
            c = self.expr('B', 2)
            e = self.expr(self.in_function, 1)
            self.emit(f'if {c[0]} {{ return {e[0]} }}', f'if ({c[1]}) {{ return {e[1]}; }}')

    def function(self):
        r = self.r
        name = self.fresh('fn')
        result = r.choice(['I', 'L', 'D', 'T', 'B'])
        params = [r.choice(['I', 'L', 'D', 'T', 'B']) for _ in range(r.randint(0, 3))]
        names = [self.fresh('a') for _ in params]
        self.m.append(f'fun {name}({", ".join(f"{n}: {MAM[t]}" for n, t in zip(names, params))}): {MAM[result]} {{')
        self.j.append(f'static {PRIM[result]} {name}({", ".join(f"{PRIM[t]} {n}" for n, t in zip(names, params))}) {{')
        saved = {t: list(v) for t, v in self.vars.items()}
        for n, t in zip(names, params):
            self.vars[t].append((n, n, False))
        self.in_function = result
        self.indent = 0
        self.block(2)
        e = self.expr(result, 1)
        self.emit(f'    return {e[0]}', f'    return {e[1]};')
        self.in_function = None
        self.vars = saved
        self.m.append('}')
        self.j.append('}')
        self.funcs.append((name, params, result))

    def program(self):
        for t, mam, java in [
            ('I', 'Int', 'int'), ('L', 'Long', 'long'), ('D', 'Double', 'double'), ('B', 'Bool', 'boolean'), ('T', 'Text', 'String')
        ]:
            self.m.append(f'fun tr{t}(x: {mam}): {mam} {{ print("t{{x}}")\n    return x }}')
            self.j.append(f'static {java} tr{t}({java} x) {{ System.out.println("t" + x); return x; }}')
        for _ in range(self.r.randint(0, 3)):
            self.function()
        self.j.append('public static void main(String[] args) {')
        for _ in range(self.r.randint(4, 9)):
            self.statement(3)
        self.j.append('}')
        return '\n'.join(self.m) + '\n', 'public class J {\n' + '\n'.join(self.j) + '\n}\n'


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 100
    here = os.path.dirname(os.path.abspath(__file__))
    work = tempfile.mkdtemp()
    problems = []
    skipped = 0
    for index in range(count):
        rng = random.Random(seed * 100000 + index)
        mam, java = Gen(rng).program()
        case = os.path.join(work, str(index))
        os.makedirs(case)
        mp = os.path.join(case, 'p.mam')
        jp = os.path.join(case, 'J.java')
        open(mp, 'w', encoding='utf-8').write(mam)
        open(jp, 'w', encoding='utf-8').write(java)
        jc = subprocess.run(['javac', '-nowarn', '-d', case, jp], capture_output=True)
        if jc.returncode != 0:
            err = jc.stderr.decode('utf-8', 'replace')
            problems.append(('JAVAC', mam, java, err))
            continue
        mc = subprocess.run([COMPILER, mp, os.path.join(case, 'out')], capture_output=True, timeout=30)
        if mc.returncode != 0:
            problems.append(('COMPILE', mam, java, mc.stderr.decode('utf-8', 'replace') + mc.stdout.decode('utf-8', 'replace')))
            continue
        try:
            a = subprocess.run(['java', '-Xverify:all', '-cp', os.path.join(case, 'out'), 'Main'], capture_output=True, timeout=20)
            b = subprocess.run(['java', '-Xverify:all', '-cp', case, 'J'], capture_output=True, timeout=20)
        except subprocess.TimeoutExpired:
            skipped += 1
            continue
        ao, bo = a.stdout.decode('utf-8', 'replace'), b.stdout.decode('utf-8', 'replace')
        ae, be = a.stderr.decode('utf-8', 'replace'), b.stderr.decode('utf-8', 'replace')
        ex = lambda s: (re.findall(r'(?:java\.lang\.\w+(?:Error|Exception))', s) or [''])[0]
        if 'VerifyError' in ae or 'ClassFormatError' in ae:
            problems.append(('VERIFY', mam, java, ae))
        elif ao != bo:
            problems.append(('OUTPUT', mam, java, f'--- mammoth\n{ao[-600:]}\n--- java\n{bo[-600:]}\n{ae[:300]}\n{be[:300]}'))
        elif (a.returncode != 0) != (b.returncode != 0) or ex(ae) != ex(be):
            problems.append(('STATUS', mam, java, f'{ae[:400]}\n---\n{be[:400]}'))
    print('programs', count, 'problems', len(problems), 'timeouts', skipped)
    counts = {}
    for p in problems:
        counts[p[0]] = counts.get(p[0], 0) + 1
    print(counts)
    for n, (kind, mam, java, detail) in enumerate(problems[:8]):
        open(os.path.join(here, f'd{n}.mam'), 'w', encoding='utf-8').write(mam)
        open(os.path.join(here, f'd{n}.java'), 'w', encoding='utf-8').write(java)
        print(kind, n, (detail.strip().splitlines() or [''])[0:3])


main()

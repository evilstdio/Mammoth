"""Soundness fuzzer. Grows programs one random, usually ill-typed statement at
a time, keeping those the compiler accepts. An accepted program must not fail
at runtime with a type or linkage error that its source does not ask for."""
import os, random, re, subprocess, sys, tempfile
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
HEADER = '''import java.util.ArrayList
import java.util.List as JList
import java.util.Collection
import java.util.Collections
import java.util.Arrays
import java.util.HashMap
import java.util.Optional
import java.util.function.Function
import java.util.function.Supplier
open class A { open fun name(): Text { return "A" } }
open class B extends A { override fun name(): Text { return "B" }
    fun onlyB(): Int { return 1 } }
class C extends A { fun onlyC(): Int { return 2 } }
class D extends B { override fun name(): Text { return "D" } }
open class Box<T>(var v: T) {
    fun get(): T { return v }
    fun set(x: T) { v = x }
    open fun swap(x: T): T { let o = v
        v = x
        return o }
}
class BBox(b: B) extends Box<B>(b) {
    override fun swap(x: B): B { x.onlyB()
        return x }
}
interface OutI<out T> { fun get(): T }
interface InI<in T> { fun put(x: T): Text }
class Out<T>(let v: T) implements OutI<T> { fun get(): T { return v } }
class In<T>(let label: Text) implements InI<T> { fun put(x: T): Text { return label } }
interface Src<T> { fun src(): T }
class SrcB implements Src<B> { fun src(): B { return B() } }
class Pair2<X, Y>(let x: X, let y: Y) { fun swap(): Pair2<Y, X> { return Pair2<Y, X>(y, x) } }
fun same<T>(x: T): T { return x }
fun pick<T>(x: T, y: T): T { return y }
fun boxOf<T>(x: T): Box<T> { return Box<T>(x) }
fun unbox<T>(b: Box<T>): T { return b.get() }
fun bounded<T: A>(x: T): T { x.name()
    return x }
fun takesA(x: A): Text { return x.name() }
fun takesB(x: B): Int { return x.onlyB() }
fun takesBoxA(x: Box<A>): A { return x.get() }
fun takesOutA(x: OutI<A>): A { return x.get() }
fun takesInB(x: InI<B>): Text { return x.put(B()) }
fun fillA(x: ArrayList<A>) { x.add(C()) }
fun nullableA(flag: Bool): A? { if flag { return A() }
    return null }
'''
TYPES = ['Any', 'Any?', 'A', 'B', 'C', 'D', 'A?', 'B?', 'Text', 'Text?', 'Int', 'Int?', 'Long', 'Double',
         'Box<A>', 'Box<B>', 'Box<C>', 'Box<Any>', 'Box<*>', 'Box<A?>', 'Box<Int>', 'Box<Text>', 'BBox',
         'Out<A>', 'Out<B>', 'OutI<A>', 'OutI<B>', 'OutI<*>', 'In<A>', 'In<B>', 'InI<A>', 'InI<B>', 'InI<*>', 'Src<A>', 'Src<B>', 'SrcB',
         'ArrayList<A>', 'ArrayList<B>', 'ArrayList<C>', 'ArrayList<Any>', 'ArrayList<Text>', 'ArrayList<Int>', 'ArrayList<A?>',
         'JList<A>', 'JList<B>', 'JList<Any>', 'JList<Int>', 'Collection<A>', 'Collection<B>',
         'MutableList<A>', 'MutableList<B>', 'MutableList<Int>', 'MutableList<Any>',
         'HashMap<Text, A>', 'HashMap<Text, B>', 'Optional<A>', 'Optional<B>', 'Supplier<A>', 'Supplier<B>', 'Function<A, B>', 'Function<B, A>',
         '(A) -> B', '(B) -> A', '(A) -> A', '() -> B', 'Pair2<A, B>', 'Pair2<B, A>', 'List<A>', 'List<B>', 'List<Int>', 'List<Any>']
ELEM = ['A', 'B', 'C', 'D', 'Any', 'Text', 'Int', 'A?', 'B?']
ATOMS = ['A()', 'B()', 'C()', 'D()', '"t"', '1', '2L', '1.5', 'null', 'true', 'BBox(B())', 'SrcB()',
         'ArrayList<A>()', 'ArrayList<B>()', 'ArrayList<Any>()', 'ArrayList<Int>()', 'MutableList<A>()', 'MutableList<B>()',
         'Box<A>(A())', 'Box<B>(B())', 'Box(B())', 'Box(C())', 'Box<Any>("s")', 'Box<Int>(3)', 'Out<B>(B())', 'Out<A>(C())', 'In<A>("i")', 'In<B>("j")',
         'Pair2<A, B>(A(), B())', 'HashMap<Text, A>()', 'HashMap<Text, B>()', 'nullableA(true)', 'nullableA(false)', '[A(), B()]', '[B(), B()]', '[1, 2]',
         '{ x: A -> x }', '{ x: B -> x }', '{ x: A -> B() }', '{ -> B() }', '{ x: B -> x.onlyB() }']


class G:
    def __init__(s, r):
        s.r, s.vars, s.n = r, [], 0

    def var(s):
        return s.r.choice(s.vars)[0] if s.vars else s.atom()

    def atom(s):
        return s.r.choice(ATOMS)

    def expr(s, d=0):
        r = s.r
        if d >= 3:
            return s.var() if r.random() < .75 else s.atom()
        k = r.choice(['var', 'var', 'var', 'var', 'atom', 'call', 'meth', 'meth', 'meth', 'java', 'ifv', 'elvis', 'req', 'cast', 'cast', 'lam', 'ctor', 'field', 'idx'])
        e = lambda: s.expr(d + 1)
        if k == 'var':
            return s.var()
        if k == 'atom':
            return s.atom()
        if k == 'call':
            f = r.choice(['same', 'pick', 'boxOf', 'unbox', 'bounded', 'takesA', 'takesB', 'takesBoxA', 'takesOutA', 'takesInB', 'fillA'])
            n = 2 if f == 'pick' else 1
            ta = f'<{r.choice(ELEM)}>' if r.random() < .25 and f in ('same', 'pick', 'boxOf', 'unbox', 'bounded') else ''
            return f'{f}{ta}({", ".join(e() for _ in range(n))})'
        if k == 'meth':
            m = r.choice(['get()', 'get(0)', 'name()', 'onlyB()', 'onlyC()', 'swap()', f'swap({e()})', f'set({e()})', f'put({e()})', f'add({e()})', f'contains({e()})', 'remove(0)', 'src()', f'apply({e()})', f'orElse({e()})', 'size()', 'stream().findFirst().get()', 'iterator().next()', 'toString()', 'hashCode()', 'getClass()'])
            if r.random() < .15:
                return f'{e()}?.{m}'
            return f'{e()}.{m}'
        if k == 'java':
            return r.choice([f'Collections.singletonList({e()})', 'Collections.emptyList()', f'Arrays.asList({e()}, {e()})', f'Arrays.asList({e()})', f'Optional.of({e()})', f'Optional.ofNullable({e()})', 'Optional.empty()', f'JList.of({e()})', f'Collections.unmodifiableList({e()})', f'ArrayList<{r.choice(ELEM)}>({e()})', f'Collections.max({e()})', f'Collections.reverse({e()})', f'Collections.addAll({e()}, {e()})', f'Collections.nCopies(1, {e()})', 'Function.identity()'])
        if k == 'ifv':
            return f'if {r.choice(["true", "false"])} {{ {e()} }} else {{ {e()} }}'
        if k == 'elvis':
            return f'({e()} ?: {e()})'
        if k == 'req':
            return f'requireValue({e()})'
        if k == 'cast':
            return f'({e()} as {r.choice(TYPES)})'
        if k == 'lam':
            t = r.choice(['A', 'B', 'C', 'Any', 'Int'])
            return f'{{ p: {t} -> {e()} }}'
        if k == 'ctor':
            t = r.choice(ELEM)
            return r.choice([f'Box<{t}>({e()})', f'Box({e()})', f'Out<{t}>({e()})', f'Out({e()})', f'In<{t}>("x")', f'Pair2({e()}, {e()})', f'BBox({e()})'])
        if k == 'field':
            return f'{e()}.{r.choice(["v", "x", "y", "label"])}'
        if k == 'idx':
            return f'{e()}[0]'
        return s.var()

    def statement(s):
        r = s.r
        s.n += 1
        name = f'v{s.n}'
        k = r.choice(['let', 'let', 'let', 'infer', 'var', 'assign', 'effect', 'effect', 'print', 'setf', 'forin'])
        if k == 'let':
            t = r.choice(TYPES)
            return f'let {name}: {t} = {s.expr()}', (name, t)
        if k == 'infer':
            return f'let {name} = {s.expr()}', (name, None)
        if k == 'var':
            t = r.choice(TYPES)
            return f'var {name}: {t} = {s.expr()}', (name, t)
        if k == 'assign' and s.vars:
            return f'{s.var()} = {s.expr()}', None
        if k == 'setf':
            return f'{s.var()}.v = {s.expr()}', None
        if k == 'forin':
            return f'for it in {s.var()} {{ print(it) }}', None
        if k == 'print':
            return f'print({s.expr()})', None
        return f'{s.expr(1)}.{r.choice(["add", "set", "put", "swap"])}({s.expr()})', None


BAD = re.compile(r'ClassCastException|VerifyError|ClassFormatError|IncompatibleClassChangeError|NoSuchMethodError|NoSuchFieldError|AbstractMethodError|BootstrapMethodError|LambdaConversionException|IllegalAccessError|ArrayStoreException|NullPointerException')
EXCUSE_NPE = ('requireValue', 'Optional.of(', 'JList.of', 'Collections.', 'Arrays.', 'ArrayList<', 'null')


def main():
    seed, count = int(sys.argv[1]), int(sys.argv[2])
    steps = int(sys.argv[3]) if len(sys.argv) > 3 else 60
    here = os.path.dirname(os.path.abspath(__file__))
    work = tempfile.mkdtemp()
    found, kept_total, panics = 0, 0, 0
    seen = set()
    for i in range(count):
        r = random.Random(seed * 1000003 + i)
        g = G(r)
        body = []
        path = os.path.join(work, 'Main.mam')
        out = os.path.join(work, 'o')
        for _ in range(steps):
            text, new = g.statement()
            program = HEADER + '\n'.join(body + [text]) + '\n'
            open(path, 'w', encoding='utf-8').write(program)
            try:
                p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=30)
            except subprocess.TimeoutExpired:
                panics += 1
                open(os.path.join(here, f'hang{seed}_{panics}.mam'), 'w', encoding='utf-8').write(program)
                print('HANG', panics)
                continue
            if p.returncode == 101 or b'panicked' in p.stderr:
                panics += 1
                fn = os.path.join(here, f'panic{seed}_{panics}.mam')
                open(fn, 'w', encoding='utf-8').write(program)
                print('PANIC', fn.split(os.sep)[-1], p.stderr.decode('utf-8', 'replace')[:300])
                continue
            if p.returncode != 0:
                continue
            run = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=20)
            err = run.stderr.decode('utf-8', 'replace')
            m = BAD.search(err)
            if m:
                lines = program.split('\n')
                where = re.search(r'at Main\.\S*\(\w+\.mam:(\d+)\)', err)
                src = lines[int(where.group(1)) - 1] if where else ''
                kind = m.group(0)
                excused = (kind == 'ClassCastException' and ' as ' in src) or (kind == 'NullPointerException' and any(x in src for x in EXCUSE_NPE))
                if not excused:
                    key = kind + re.sub(r'v\d+', 'v', src)
                    if key not in seen:
                        seen.add(key)
                        found += 1
                        fn = os.path.join(here, f's{seed}_{found}.mam')
                        open(fn, 'w', encoding='utf-8').write(program)
                        detail = err.strip().splitlines()
                        print('BAD', kind, fn.split(os.sep)[-1], '|', src[:150], '|', detail[1][:150] if len(detail) > 1 else detail[:1])
                continue
            if run.returncode != 0:
                continue
            body.append(text)
            kept_total += 1
            if new:
                g.vars.append(new)
    print('programs', count, 'kept statements', kept_total, 'bad', found, 'panics', panics)


main()

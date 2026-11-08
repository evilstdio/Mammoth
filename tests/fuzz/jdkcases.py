"""Common JDK usage: each snippet is valid in the Java sense and should compile and run."""
import os, subprocess, shutil, sys, tempfile
HERE = tempfile.mkdtemp().replace('\\', '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
IMPORTS = '''import java.util.ArrayList
import java.util.Arrays
import java.util.Collections
import java.util.Comparator
import java.util.HashMap
import java.util.HashSet
import java.util.LinkedHashMap
import java.util.LinkedList
import java.util.ArrayDeque
import java.util.TreeMap
import java.util.PriorityQueue
import java.util.Optional
import java.util.StringJoiner
import java.util.List as JList
import java.util.Map as JMap
import java.util.Set as JSet
import java.util.stream.Collectors
import java.util.stream.IntStream
import java.util.stream.Stream
import java.util.function.Function
import java.util.function.BiFunction
import java.util.function.Predicate
import java.util.function.Supplier
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executors
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import java.util.regex.Pattern
import java.math.BigDecimal
import java.math.BigInteger
import java.time.LocalDate
import java.time.Duration
import java.nio.file.Files
import java.nio.file.Path
import java.nio.charset.StandardCharsets
'''
PRE = '''let names = ArrayList<Text>()
names.add("bob")
names.add("alice")
names.add("carol")
let nums = ArrayList<Int>()
nums.add(3)
nums.add(1)
nums.add(2)
'''
cases = {
'stream_map_collect': 'let u = names.stream().map { it.toUpperCase() }.collect(Collectors.toList())\nprint(u.get(0).length())\n',
'stream_filter_count': 'print(names.stream().filter { it.length() > 3 }.count())\n',
'stream_sorted_join': 'print(names.stream().sorted().collect(Collectors.joining(",")))\n',
'stream_grouping': 'let g = names.stream().collect(Collectors.groupingBy { x: Text -> x.length() })\nprint(g.get(3).size())\n',
'stream_tomap': 'let m = names.stream().collect(Collectors.toMap({ x: Text -> x }, { x: Text -> x.length() }))\nprint(m.get("bob") + 1)\n',
'stream_reduce': 'print(nums.stream().reduce(0) { a, b -> a + b })\n',
'stream_maptoint_sum': 'print(nums.stream().mapToInt { it * 2 }.sum())\n',
'stream_flatmap': 'print(names.stream().flatMap { x -> Stream.of(x, x) }.count())\n',
'stream_anymatch': 'print(names.stream().anyMatch { it == "bob" })\n',
'stream_foreach': 'names.stream().forEach { print(it) }\n',
'stream_method_chain_multi': 'print(names.stream()\n  .filter { it.length() > 3 }\n  .map { it.length() }\n  .collect(Collectors.toList())\n  .size())\n',
'stream_min_comparator': 'print(names.stream().min(Comparator.naturalOrder()).get())\n',
'intstream_range': 'print(IntStream.range(0, 5).sum())\n',
'intstream_boxed': 'let b = IntStream.range(0, 3).boxed().collect(Collectors.toList())\nprint(b.get(0) + 1)\n',
'stream_of': 'print(Stream.of("a", "b").count())\n',
'arrays_stream': 'print(Arrays.stream([1, 2, 3]).sum())\n',
'arrays_sort': 'let a = [3, 1, 2]\nArrays.sort(a)\nprint(a[0])\n',
'arrays_tostring': 'print(Arrays.toString([1, 2]))\n',
'arrays_aslist': 'let l = Arrays.asList("a", "b")\nprint(l.get(1).length())\n',
'collections_sort': 'Collections.sort(names)\nprint(names.get(0))\n',
'collections_sort_cmp': 'Collections.sort(names, Comparator.comparing { x: Text -> x.length() })\nprint(names.get(0))\n',
'list_sort_lambda': 'names.sort { a, b -> a.length() - b.length() }\nprint(names.get(0))\n',
'comparator_then': 'names.sort(Comparator.comparing({ x: Text -> x.length() }).thenComparing({ x: Text -> x }))\nprint(names.get(0))\n',
'comparator_reversed': 'names.sort(Comparator.naturalOrder<Text>().reversed())\nprint(names.get(0))\n',
'comparator_comparingint': 'names.sort(Comparator.comparingInt { x: Text -> x.length() })\nprint(names.get(0))\n',
'collections_max': 'print(Collections.max(nums) + 1)\n',
'list_removeif': 'names.removeIf { it.length() == 3 }\nprint(names.size())\n',
'list_foreach': 'names.forEach { print(it) }\n',
'list_replaceall': 'names.replaceAll { it + "!" }\nprint(names.get(0))\n',
'list_of_get': 'print(JList.of(1, 2).get(0) + 1)\n',
'list_copy': 'print(JList.copyOf(names).size())\n',
'list_tostring': 'print(names.toString().length() > 0)\n',
'list_toarray': 'let a = names.toArray()\nprint(a.size())\n',
'list_iterator': 'let it2 = names.iterator()\nwhile it2.hasNext() { print(it2.next().length()) }\n',
'list_sublist': 'print(names.subList(0, 1).size())\n',
'list_indexof': 'print(names.indexOf("bob"))\n',
'map_basic': 'let m = HashMap<Text, Int>()\nm.put("a", 1)\nprint(m.get("a") + 1)\n',
'map_getordefault': 'let m = HashMap<Text, Int>()\nprint(m.getOrDefault("a", 5) + 1)\n',
'map_merge': 'let m = HashMap<Text, Int>()\nm.merge("a", 1) { x, y -> x + y }\nprint(m.get("a"))\n',
'map_computeifabsent': 'let m = HashMap<Text, ArrayList<Int>>()\nm.computeIfAbsent("a") { k -> ArrayList<Int>() }.add(1)\nprint(m.get("a").size())\n',
'map_compute': 'let m = HashMap<Text, Int>()\nm.compute("a") { k, v -> if v == null { 1 } else { v + 1 } }\nprint(m.get("a"))\n',
'map_entryset': 'let m = HashMap<Text, Int>()\nm.put("a", 1)\nfor e in m.entrySet() { print(e.getKey() + "{e.getValue() + 1}") }\n',
'map_foreach': 'let m = HashMap<Text, Int>()\nm.put("a", 1)\nm.forEach { k, v -> print(k + "{v}") }\n',
'map_keyset_stream': 'let m = HashMap<Text, Int>()\nm.put("a", 1)\nprint(m.keySet().stream().count())\n',
'map_values_sum': 'let m = HashMap<Text, Int>()\nm.put("a", 1)\nvar t = 0\nfor v in m.values() { t += v }\nprint(t)\n',
'map_of': 'print(JMap.of("a", 1).get("a") + 1)\n',
'treemap_first': 'let t = TreeMap<Text, Int>()\nt.put("a", 1)\nprint(t.firstKey().length())\n',
'linkedhashmap': 'let m = LinkedHashMap<Text, Int>()\nm.put("a", 1)\nprint(m.size())\n',
'set_basic': 'let s = HashSet<Text>()\ns.add("a")\nprint(s.contains("a"))\n',
'set_of': 'print(JSet.of(1).size())\n',
'set_from_list': 'print(HashSet<Text>(names).size())\n',
'deque': 'let d = ArrayDeque<Int>()\nd.push(1)\nprint(d.pop() + 1)\n',
'linkedlist': 'let d = LinkedList<Text>()\nd.addFirst("a")\nprint(d.getFirst().length())\n',
'pq': 'let q = PriorityQueue<Int>()\nq.add(3)\nprint(q.poll() + 1)\n',
'pq_cmp': 'let q = PriorityQueue<Int>(Comparator.reverseOrder())\nq.add(3)\nprint(q.peek())\n',
'optional_map': 'print(Optional.of("a").map { it.length() }.orElse(0) + 1)\n',
'optional_empty': 'let o: Optional<Text> = Optional.empty()\nprint(o.isPresent())\n',
'optional_ifpresent': 'Optional.of("a").ifPresent { print(it) }\n',
'optional_orelseget': 'print(Optional.empty<Text>().orElseGet { "z" }.length())\n',
'optional_orelsethrow': 'print(Optional.of("a").orElseThrow().length())\n',
'sb': 'let b = StringBuilder()\nb.append("a").append(1).append(true)\nprint(b.toString())\n',
'sb_reverse': 'print(StringBuilder("ab").reverse().toString())\n',
'string_format': 'print(Text.format("%d-%s", 1, "a"))\n',
'string_join': 'print(String.join(",", names))\n',
'string_valueof': 'print(String.valueOf(1).length())\n',
'string_split': 'print("a,b".split(",").size())\n',
'string_chars': 'print("ab".chars().count())\n',
'string_replace': 'print("ab".replace("a", "c"))\n',
'string_contains': 'print("ab".contains("a"))\n',
'string_substring': 'print("abc".substring(1, 2))\n',
'string_trim_strip': 'print(" a ".trim().strip().isEmpty())\n',
'string_compare': 'print("a".compareTo("b"))\n',
'string_charat': 'print("ab".charAt(0))\n',
'string_repeat': 'print("a".repeat(2))\n',
'integer_parse': 'print(Integer.parseInt("5") + 1)\n',
'integer_max': 'print(Integer.MAX_VALUE)\n',
'integer_tostring': 'print(Integer.toString(5).length())\n',
'integer_compare': 'print(Integer.compare(1, 2))\n',
'long_parse': 'print(Long.parseLong("5") + 1L)\n',
'double_parse': 'print(Double.parseDouble("1.5") + 1.0)\n',
'math': 'print(Math.max(1, 2) + Math.abs(-1))\n',
'math_sqrt': 'print(Math.sqrt(4.0))\n',
'math_floor_int': 'print(Math.floorMod(5, 3))\n',
'math_pow_mixed': 'print(Math.pow(2, 3))\n',
'bigdecimal': 'print(BigDecimal("1.5").add(BigDecimal.ONE).toString())\n',
'biginteger': 'print(BigInteger.valueOf(5).multiply(BigInteger.TEN).intValue() + 1)\n',
'localdate': 'print(LocalDate.of(2020, 1, 2).plusDays(1).getDayOfMonth() + 1)\n',
'duration': 'print(Duration.ofSeconds(60).toMinutes() + 1L)\n',
'regex': 'let m = Pattern.compile("a+").matcher("caab")\nprint(m.find())\nprint(m.group().length())\n',
'atomic': 'let a = AtomicInteger(1)\nprint(a.incrementAndGet() + 1)\n',
'chm': 'let m = ConcurrentHashMap<Text, Int>()\nm.put("a", 1)\nprint(m.get("a") + 1)\n',
'future': 'let f = CompletableFuture.supplyAsync { "x" }.thenApply { it.length() }\nprint(f.get() + 1)\n',
'executor': 'let e = Executors.newFixedThreadPool(1)\nlet f = e.submit { "x" }\nprint(f.get())\ne.shutdown()\n',
'thread': 'let t = Thread { print("t") }\nt.start()\nt.join()\n',
'runnable_var': 'let r: Runnable = Runnable { print("r") }\nr.run()\n',
'function_var': 'let f: Function<Text, Int> = Function { x -> x.length() }\nprint(f.apply("ab") + 1)\n',
'function_andthen': 'let f: Function<Text, Int> = Function { x -> x.length() }\nprint(f.andThen { x -> x + 1 }.apply("ab") + 1)\n',
'predicate_negate': 'let p: Predicate<Text> = Predicate { x -> x.isEmpty() }\nprint(p.negate().test("a"))\n',
'bifunction': 'let f: BiFunction<Int, Int, Int> = BiFunction { a, b -> a + b }\nprint(f.apply(1, 2) + 1)\n',
'supplier': 'let s: Supplier<Text> = Supplier { "a" }\nprint(s.get().length())\n',
'files': 'let p = Files.createTempFile("m", ".txt")\nFiles.writeString(p, "hi")\nprint(Files.readString(p).length())\nFiles.delete(p)\n',
'files_lines': 'let p = Files.createTempFile("m", ".txt")\nFiles.write(p, JList.of("a", "b"))\nprint(Files.readAllLines(p).size())\nFiles.delete(p)\n',
'path_of': 'print(Path.of("a", "b").getFileName().toString())\n',
'charset': 'print("a".getBytes(StandardCharsets.UTF_8).size())\n',
'string_joiner': 'let j = StringJoiner(",")\nj.add("a").add("b")\nprint(j.toString())\n',
'system_props': 'print(System.getProperty("java.version") != null)\n',
'system_nanotime': 'print(System.nanoTime() > 0L)\n',
'system_getenv': 'print(System.getenv().size() >= 0)\n',
'object_hash': 'print(java.util.Objects.hash(1, "a") != 0)\n',
'objects_equals': 'print(java.util.Objects.equals("a", "a"))\n',
'exception_message': 'try { throw IllegalArgumentException("m") } catch (e: IllegalArgumentException) { print(e.getMessage()) }\n',
'exception_cause': 'let e = RuntimeException("o", IllegalStateException("c"))\nprint(e.getCause().getMessage())\n',
'custom_exception': 'class E(m: Text) extends RuntimeException(m)\ntry { throw E("x") } catch (e: E) { print(e.getMessage()) }\n',
'iterable_impl': 'import java.util.Iterator\nclass R implements Iterable<Int> {\n fun iterator(): Iterator<Int> { return JList.of(1, 2).iterator() }\n}\nfor x in R() { print(x + 1) }\n',
'comparable_impl': 'class V(let n: Int) implements Comparable<V> {\n fun compareTo(o: V): Int { return n - o.n }\n}\nlet l = ArrayList<V>()\nl.add(V(2))\nl.add(V(1))\nCollections.sort(l)\nprint(l.get(0).n)\n',
'runnable_impl': 'class W implements Runnable { fun run() { print("w") } }\nThread(W()).start()\n',
'equals_override': 'class Q(let n: Int) { override fun equals(o: Any?): Bool { return o is Q and o.n == n }\n override fun hashCode(): Int { return n } }\nlet s = HashSet<Q>()\ns.add(Q(1))\nprint(s.contains(Q(1)))\n',
'tostring_override': 'class Q { override fun toString(): Text { return "q" } }\nprint(Q())\nprint("{Q()}")\n',
'data_in_set': 'data class P(let a: Int)\nlet s = HashSet<P>()\ns.add(P(1))\nprint(s.contains(P(1)))\n',
'enum_values_java': 'enum E { A, B }\nprint(E.values().size())\nprint(E.valueOf("A"))\nprint(E.A.name().length())\nprint(E.B.ordinal())\n',
'enum_map': 'import java.util.EnumMap\nenum E { A }\nlet m = EnumMap<E, Int>(E.class)\nm.put(E.A, 1)\nprint(m.get(E.A) + 1)\n',
'char_methods': "print(Character.isDigit('5'))\nprint(Character.toUpperCase('a'))\n",
}
out_dir = os.path.join(HERE, 'jdk')
os.makedirs(out_dir, exist_ok=True)
ok, fail = 0, 0
only = sys.argv[1:]
for name, src in cases.items():
    if only and name not in only:
        continue
    path = os.path.join(out_dir, name + '.mam')
    head = ''
    body = src
    lines = src.split('\n')
    extra = [l for l in lines if l.startswith('import ')]
    body = '\n'.join(l for l in lines if not l.startswith('import '))
    open(path, 'w', encoding='utf-8').write(IMPORTS + '\n'.join(extra) + '\n' + PRE + body)
    out = os.path.join(out_dir, 'o')
    shutil.rmtree(out, ignore_errors=True)
    p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=30)
    if p.returncode != 0:
        lines = p.stderr.decode('utf-8', 'replace').strip().splitlines()
        msg = lines[0].split(': ', 1)[-1] if lines else f'exit {p.returncode}'
        loc = lines[0].split('.mam:')[-1].split(': ')[0] if lines else ''
        print(f'REJ  {name}: {msg[:110]}  [{lines[1].strip()[:70] if len(lines) > 1 else ""}]')
        fail += 1
        continue
    r = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=30)
    if r.returncode != 0:
        print(f'RUN  {name}: {r.stderr.decode("utf-8", "replace").strip().splitlines()[0][:170]}')
        fail += 1
    else:
        ok += 1
print('ok', ok, 'fail', fail)

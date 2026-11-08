package lib;
public class Holder<T> {
    protected T value;
    protected java.util.List<T> history = new java.util.ArrayList<>();
    protected final String label = "holder";
    public Holder(T start) { value = start; }
    protected T current() { return value; }
    public static <U> U choose(U first, U second) { return first; }
    public static <U> void anchored(java.util.List<U> values, U value) { values.add(value); }
    public static <U> void anchoredReverse(U value, java.util.List<U> values) { values.add(value); }
    public static <A, B extends A> B dependentAnchored(java.util.List<A> anchor, B value) { return value; }
    public static <A, B extends java.util.List<A>> A nestedDependent(B values) { return values.get(0); }
    public static <A, B extends A, C extends B> C chain(A first, B second, C third) { return third; }
    public static <U extends CharSequence> U boundedCommon(U first, U second) { return first; }
    public <U> java.util.List<U> contextualEmpty() { return new java.util.ArrayList<>(); }
    public <U extends Number> java.util.List<U> numericEmpty() { return new java.util.ArrayList<>(); }
    public static <U> java.util.List<U> listIdentity(java.util.List<U> input) { return input; }
    public static <U extends Number> U supplied(java.util.function.Supplier<U> source) { return source.get(); }
    public static <U> U either(java.util.function.Supplier<U> first, java.util.function.Supplier<U> second) { return first.get(); }
    public static <U> U withValue(U input, java.util.function.Supplier<U> source) { return source.get(); }
    public interface Parent<A, B> { B apply(A value); }
    public interface Child<X> extends Parent<String, X> { }
    public interface Swapped<A, B> extends Parent<B, A> { }
    public static <U> U child(Child<U> function) { return function.apply("abcd"); }
    public static <A, B> A swapped(Swapped<A, B> function, B input) { return function.apply(input); }
    public static <A, B> java.util.function.Function<A, B> make(java.util.function.Function<A, B> function) { return function; }
    public static class Numeric<N extends Number> {
        public Numeric() { }
        public int number(N value) { return value.intValue(); }
    }
    public interface Ordered<N extends Number & Comparable<N>> { }
    public static class Dependent<A, B extends A> { public Dependent() { } }
    public static class Recursive<R extends Comparable<R>> { public Recursive() { } }
    public static String overloaded(java.util.List<String> values) { return "strings"; }
    public static String overloaded(java.util.Collection<Integer> values) { return "ints"; }
    public static String reversed(java.util.Collection<Integer> values) { return "ints"; }
    public static String reversed(java.util.List<String> values) { return "strings"; }
    public static <U extends Number & Comparable<U>> String boundedOverload(U value) { return "ordered"; }
    public static String boundedOverload(Object value) { return "object"; }
    public static class Choice {
        private String kind;
        public Choice(java.util.List<String> values) { kind = "strings"; }
        public Choice(java.util.Collection<Integer> values) { kind = "ints"; }
        public String kind() { return kind; }
    }
    public static class ParentChoice {
        public String pick(java.util.Collection<Integer> values) { return "parent"; }
    }
    public static class ChildChoice extends ParentChoice {
        public String pick(java.util.List<String> values) { return "child"; }
    }
    public static String spread(java.util.List<String> values) { return "fixed"; }
    public static String spread(java.util.Collection<Integer>... values) { return "spread"; }
    public static String annotated(java.util.function.Consumer<Integer> function) { function.accept(3); return "consumer"; }
    public static String annotated(java.util.function.Function<String, Integer> function) { return "function" + function.apply("abc"); }
    public static int numberReturn(java.util.function.Function<String, Number> function) { return function.apply("abc").intValue(); }
    public static String specific(java.util.Collection<String> values) { return "collection"; }
    public static String specific(java.util.List<String> values) { return "list"; }
    public static String specificReverse(java.util.List<String> values) { return "list"; }
    public static String specificReverse(java.util.Collection<String> values) { return "collection"; }
    public static String nullable(Object value) { return "object"; }
    public static String nullable(String value) { return "string"; }
    public static String arrays(Object[] value) { return "objects"; }
    public static String arrays(String[] value) { return "strings"; }
    public static String emptySpread(Object... values) { return "objects"; }
    public static String emptySpread(String... values) { return "strings"; }
    public static String numericTie(double value) { return "double"; }
    public static String numericTie(float value) { return "float"; }
    public static void ambiguous(StringBuilder value) { }
    public static void ambiguous(StringBuffer value) { }
    public static void crossed(CharSequence left, Object right) { }
    public static void crossed(Object left, CharSequence right) { }
    public static void unrelated(java.io.Serializable value) { }
    public static void unrelated(CharSequence value) { }
    public static class SpecificConstructor {
        private String kind;
        public SpecificConstructor(Object value) { kind = "object"; }
        public SpecificConstructor(String value) { kind = "string"; }
        public String kind() { return kind; }
    }
    public static String primitiveArray(Object value) { return "object"; }
    public static String primitiveArray(int[] value) { return "ints"; }
    public static void arrayAmbiguous(int[] value) { }
    public static void arrayAmbiguous(Object[] value) { }
    public interface TextSource { String get(); }
    public interface IntSource { int get(); }
    public interface TextMapper { int apply(String value); }
    public interface IntMapper { int apply(Integer value); }
    public static String byResult(TextSource source) { return "text:" + source.get(); }
    public static String byResult(IntSource source) { return "int:" + source.get(); }
    public static String byResultReverse(IntSource source) { return "int:" + source.get(); }
    public static String byResultReverse(TextSource source) { return "text:" + source.get(); }
    public static int byBody(IntMapper mapper) { return mapper.apply(12); }
    public static int byBody(TextMapper mapper) { return mapper.apply("abc"); }
    public interface OtherIntSource { int get(); }
    public static void bothBodies(IntSource source) { source.get(); }
    public static void bothBodies(OtherIntSource source) { source.get(); }
    public T read() { return value; }
    public static <U extends Comparable<U>> U self(U input) { return input; }
    public static <U extends AutoCloseable> U closeOnly(U input) { return input; }
    public <T> T shadow(T input) { return input; }
    public static <K, V> K key(java.util.Map<K, V> values) { return values.keySet().iterator().next(); }
    public static <U extends Number & Comparable<U>> U ordered(U value) { return value; }
    public static <U extends Runnable & AutoCloseable> U intersection(U value) { return value; }
    public static <A, B extends A> B dependent(A first, B second) { return second; }

    public void exact(java.util.Collection<T> values) { history.addAll(values); }
    public void upper(java.util.Collection<? extends T> values) { history.addAll(values); }
    public void lower(java.util.Collection<? super T> values) { values.add(value); }
    public static void nestedUpper(java.util.List<java.util.List<? extends Number>> values) { }
    public static void nestedCovariant(java.util.List<? extends java.util.List<? extends Number>> values) { }
    public static void nested(java.util.List<java.util.List<String>> values) { }

    // Overloads whose nested factory arguments need each candidate's context.
    public static String nest(java.util.List<java.util.List<String>> values) { return "lists" + values.get(0).size(); }
    public static String nest(String value) { return "text"; }
    public static String nestReverse(String value) { return "text"; }
    public static String nestReverse(java.util.List<java.util.List<String>> values) { return "lists" + values.get(0).size(); }
    public static String shapes(java.util.List<Integer> values) { return "list" + values.size(); }
    public static String shapes(java.util.Map<String, Integer> values) { return "map" + values.size(); }
    public static String paired(java.util.List<String> values, int count) { return "int"; }
    public static String paired(java.util.List<String> values, String label) { return "text"; }
    public static String nestedSpread(String head, java.util.List<String>... tails) { return "spread" + tails.length; }
    public static String nestedSpread(int head) { return "int"; }
    public static <U> java.util.List<U> wrap(U value) { java.util.List<U> list = new java.util.ArrayList<>(); list.add(value); return list; }
    public static <U> java.util.List<U> wrap(U first, U second) { java.util.List<U> list = new java.util.ArrayList<>(); list.add(first); list.add(second); return list; }
    public static String deep(java.util.List<java.util.List<java.util.List<String>>> values) { return "deep" + values.get(0).get(0).size(); }
    public static String deep(int value) { return "int"; }
    public static class NestChoice {
        public final String kind;
        public NestChoice(java.util.List<java.util.Set<String>> values) { kind = "sets"; }
        public NestChoice(String value) { kind = "text"; }
    }
    public String nestInstance(java.util.List<java.util.Set<String>> values) { return "sets"; }
    public String nestInstance(Integer value) { return "int"; }
    public static String lowerOverload(java.util.List<? super Integer> values) { values.add(3); return "lower" + values.size(); }
    public static String lowerOverload(String value) { return "text"; }
    public static void lowerOnly(java.util.List<? super Integer> values) { values.add(4); }

    // Wildcard results and fields: readable as their bound, never writable.
    public static java.util.List<Integer> backing = new java.util.ArrayList<>(java.util.List.of(1, 2));
    public static java.util.List<? extends Number> numbers() { return backing; }
    public static java.util.List<?> unknown() { return backing; }
    public static java.util.List<? super Integer> sink() { return new java.util.ArrayList<Object>(java.util.List.of("s")); }
    public static java.util.Map<String, ? extends Number> numberMap() { return java.util.Map.of("a", 1); }
    public static java.util.List<? extends Number> wildField = backing;
    public static java.util.List<Integer> plainField = backing;
    public java.util.List<? extends CharSequence> texts = new java.util.ArrayList<>(java.util.List.of("t"));
    public static <U> void pour(java.util.List<U> into, java.util.List<U> from) { into.add(from.get(0)); }
    // Specificity compares type arguments as Java does.
    public static String genericTie(java.util.List<String> values) { return "list-strings"; }
    public static String genericTie(java.util.ArrayList<Integer> values) { return "arraylist-ints"; }
    public static String genericAgree(java.util.List<String> values) { return "list"; }
    public static String genericAgree(java.util.ArrayList<String> values) { return "arraylist"; }
    public static <U> String genericVariable(java.util.List<U> values) { return "list-U"; }
    public static String genericVariable(java.util.ArrayList<Integer> values) { return "arraylist-ints"; }
    public static final String PUBLIC_LABEL = "public";
    protected static java.util.List<String> protectedNames = new java.util.ArrayList<>(java.util.List.of("p"));
    protected static String shapeOf(int width, int height) { return "shape" + (width * height); }
    public static class SubHolder extends Holder<String> {
        public SubHolder() { super("sub"); }
    }
    public static String[] holes() { return new String[] { "first", null }; }
    public static String crossed(java.util.List<?> a, Object b) { return "list-object"; }
    public static String crossed(java.util.Collection<?> a, java.util.List<?> b) { return "collection-list"; }
    public static String phases(Object a) { return "object"; }
    public static String phases(int... a) { return "varargs"; }
    public static String boxingLater(long a) { return "long"; }
    public static String boxingLater(Integer a) { return "integer"; }
    public static String narrowest(Iterable<?> a) { return "iterable"; }
    public static String narrowest(java.util.Collection<?> a) { return "collection"; }
    public static int backingSum() { int sum = 0; for (Integer value : backing) sum += value; return sum; }

}

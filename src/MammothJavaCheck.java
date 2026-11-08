import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.lang.invoke.MethodType;
import java.lang.reflect.Modifier;
import java.util.Arrays;

class MammothJavaCheck {
    public static void main(String[] args) throws Exception {
        var input = new BufferedReader(new InputStreamReader(System.in));
        var declarations = input.lines().toList();
        var loader = ClassLoader.getSystemClassLoader();
        for (int index = 0; index < declarations.size(); index++) {
            var declaration = declarations.get(index).split("\t", -1);
            try {
                var error = check(declaration, loader);
                if (error != null) {
                    report(index, error);
                }
            } catch (ClassNotFoundException | TypeNotPresentException error) {
                report(index, "Java type was not found on the classpath: " + error.getMessage());
            } catch (LinkageError | IllegalArgumentException | SecurityException error) {
                report(index, "Could not inspect the Java declaration: " + error);
            }
        }
    }

    private static String check(String[] declaration, ClassLoader loader) throws ClassNotFoundException {
        var operation = declaration[0];
        var name = declaration[1].replace('/', '.');
        var member = declaration[2];
        var descriptor = declaration[3];
        var type = Class.forName(name, false, loader);

        if (!Modifier.isPublic(type.getModifiers()) ||
            !type.getModule().isExported(type.getPackageName(), MammothJavaCheck.class.getModule())) {
            return "Java type is not publicly accessible: " + name;
        }

        switch (operation) {
            case "class", "interface" -> {
                if (type.isInterface() != operation.equals("interface")) {
                    return name + " is a Java " + (type.isInterface() ? "interface" : "class") + ".";
                }
            }
            case "constructor" -> {
                if (Modifier.isAbstract(type.getModifiers())) {
                    return "Cannot construct the abstract Java type " + name + ".";
                }
                var signature = MethodType.fromMethodDescriptorString(descriptor, loader);
                try {
                    type.getConstructor(signature.parameterArray());
                } catch (NoSuchMethodException error) {
                    return "No public Java constructor matches " + name + signature + ".";
                }
            }
            case "field" -> {
                var expected = MethodType.fromMethodDescriptorString("()" + descriptor, loader).returnType();
                var found = Arrays.stream(type.getFields()).anyMatch(field ->
                    field.getName().equals(member) && field.getType() == expected &&
                    Modifier.isStatic(field.getModifiers()));
                if (!found) {
                    return "No public static Java field matches " + name + "." + member +
                        " with type " + expected.getTypeName() + ".";
                }
            }
            case "static", "static-interface", "instance" -> {
                var isStatic = !operation.equals("instance");
                if (isStatic && type.isInterface() != operation.equals("static-interface")) {
                    return "Declare " + name + " as a Java interface before binding its static method.";
                }
                var signature = MethodType.fromMethodDescriptorString(descriptor, loader);
                var found = Arrays.stream(type.getMethods()).anyMatch(method ->
                    method.getName().equals(member) && method.getReturnType() == signature.returnType() &&
                    Arrays.equals(method.getParameterTypes(), signature.parameterArray()) &&
                    Modifier.isStatic(method.getModifiers()) == isStatic);
                if (!found) {
                    return "No public " + (isStatic ? "static" : "instance") + " Java method matches " +
                        name + "." + member + signature + ".";
                }
            }
            default -> throw new IllegalArgumentException("Unknown operation: " + operation);
        }
        return null;
    }

    private static void report(int index, String message) {
        System.out.println(index + "\t" + message.replace('\n', ' ').replace('\r', ' '));
    }
}

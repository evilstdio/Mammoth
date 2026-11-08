package benchmark.java;

public final class JavaChecksum {
    public static int checksum(int limit) {
        int index = 0;
        int result = 0;
        while (index < limit) {
            result = (result * 31 + index) % 1000003;
            index = index + 1;
        }
        return result;
    }
}

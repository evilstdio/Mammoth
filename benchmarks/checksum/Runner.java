import java.util.function.IntUnaryOperator;
import benchmark.java.JavaChecksum;
import benchmark.kotlin.KotlinChecksumKt;
import benchmark.mammoth.MammothChecksum;

public final class Runner {
    public static void main(String[] args) {
        IntUnaryOperator checksum = switch (args[0]) {
            case "Mammoth" -> MammothChecksum::checksum;
            case "Java" -> JavaChecksum::checksum;
            case "Kotlin" -> KotlinChecksumKt::checksum;
            default -> throw new IllegalArgumentException(args[0]);
        };

        if (args[1].equals("cold")) {
            System.out.println(checksum.applyAsInt(1000));
            return;
        }
        if (checksum.applyAsInt(1_000_000) != 256782) {
            throw new AssertionError("Wrong checksum");
        }
        long total = 0;
        for (int index = 0; index < 40; index++) {
            total += checksum.applyAsInt(1_000_000 + index);
        }
        for (int sample = 0; sample < 10; sample++) {
            long start = System.nanoTime();
            for (int index = 0; index < 8; index++) {
                total += checksum.applyAsInt(1_000_000 + sample * 8 + index);
            }
            long elapsed = System.nanoTime() - start;
            System.out.println(elapsed / 8);
        }
        System.out.println(total);
    }
}

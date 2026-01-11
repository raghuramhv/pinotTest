/**
 * Simple Java benchmark to compare against Rust implementations
 * This runs equivalent operations to what we benchmark in Rust
 */
import java.util.*;
import java.util.concurrent.TimeUnit;

public class JavaBenchmark {

    // Configuration
    static final int WARMUP_ITERATIONS = 5;
    static final int MEASURE_ITERATIONS = 10;
    static final int ARRAY_SIZE = 10_000;
    static final int DICT_SIZE = 1000;
    static final int BITMAP_SIZE = 5_000;

    public static void main(String[] args) {
        System.out.println("=== Java Benchmark Results ===\n");

        // Aggregation benchmarks
        benchmarkSum();
        benchmarkCount();
        benchmarkMinMax();
        benchmarkAvg();

        // Dictionary benchmarks
        benchmarkDictLookup();
        benchmarkDictDecode();
        benchmarkMutableDictInsert();

        // Bitmap benchmarks (using BitSet as stand-in, RoaringBitmap would be fairer)
        benchmarkBitmapAnd();
        benchmarkBitmapOr();
        benchmarkBitmapIteration();

        System.out.println("\n=== Java Benchmark Complete ===");
    }

    // ============== Aggregation Benchmarks ==============

    static void benchmarkSum() {
        double[] values = new double[ARRAY_SIZE];
        Random rand = new Random(42);
        for (int i = 0; i < values.length; i++) {
            values[i] = rand.nextDouble() * 1000;
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            sumArray(values);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            sumArray(values);
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("sum_10k:                  %.3f µs%n", avgMicros);
    }

    static double sumArray(double[] values) {
        double sum = 0;
        for (double v : values) {
            sum += v;
        }
        return sum;
    }

    static void benchmarkCount() {
        double[] values = new double[ARRAY_SIZE];

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            countArray(values);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            countArray(values);
            totalNanos += System.nanoTime() - start;
        }

        double avgNanos = totalNanos / (double) MEASURE_ITERATIONS;
        System.out.printf("count_10k:                %.3f ns%n", avgNanos);
    }

    static long countArray(double[] values) {
        return values.length;
    }

    static void benchmarkMinMax() {
        double[] values = new double[ARRAY_SIZE];
        Random rand = new Random(42);
        for (int i = 0; i < values.length; i++) {
            values[i] = rand.nextDouble() * 1000;
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            minArray(values);
            maxArray(values);
        }

        // Measure MIN
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            minArray(values);
            totalNanos += System.nanoTime() - start;
        }
        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("min_max/min:              %.3f µs%n", avgMicros);

        // Measure MAX
        totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            maxArray(values);
            totalNanos += System.nanoTime() - start;
        }
        avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("min_max/max:              %.3f µs%n", avgMicros);
    }

    static double minArray(double[] values) {
        double min = Double.MAX_VALUE;
        for (double v : values) {
            if (v < min) min = v;
        }
        return min;
    }

    static double maxArray(double[] values) {
        double max = Double.MIN_VALUE;
        for (double v : values) {
            if (v > max) max = v;
        }
        return max;
    }

    static void benchmarkAvg() {
        double[] values = new double[ARRAY_SIZE];
        Random rand = new Random(42);
        for (int i = 0; i < values.length; i++) {
            values[i] = rand.nextDouble() * 1000;
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            avgArray(values);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            avgArray(values);
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("avg_10k:                  %.3f µs%n", avgMicros);
    }

    static double avgArray(double[] values) {
        double sum = 0;
        for (double v : values) {
            sum += v;
        }
        return sum / values.length;
    }

    // ============== Dictionary Benchmarks ==============

    static void benchmarkDictLookup() {
        // Build dictionary
        Map<Integer, Integer> dict = new HashMap<>();
        int[] values = new int[DICT_SIZE];
        for (int i = 0; i < DICT_SIZE; i++) {
            values[i] = i * 3;
            dict.put(i * 3, i);
        }

        // Lookup values
        int[] lookups = new int[1000];
        Random rand = new Random(42);
        for (int i = 0; i < lookups.length; i++) {
            lookups[i] = values[rand.nextInt(DICT_SIZE)];
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            dictLookup(dict, lookups);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            dictLookup(dict, lookups);
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("int_dict_lookup:          %.3f µs%n", avgMicros);
    }

    static int dictLookup(Map<Integer, Integer> dict, int[] lookups) {
        int sum = 0;
        for (int v : lookups) {
            Integer idx = dict.get(v);
            if (idx != null) sum += idx;
        }
        return sum;
    }

    static void benchmarkDictDecode() {
        // Build dictionary
        int[] values = new int[DICT_SIZE];
        for (int i = 0; i < DICT_SIZE; i++) {
            values[i] = i * 3;
        }

        // Decode indices
        int[] indices = new int[1000];
        Random rand = new Random(42);
        for (int i = 0; i < indices.length; i++) {
            indices[i] = rand.nextInt(DICT_SIZE);
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            dictDecode(values, indices);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            dictDecode(values, indices);
            totalNanos += System.nanoTime() - start;
        }

        double avgNanos = totalNanos / (double) MEASURE_ITERATIONS;
        System.out.printf("int_dict_decode:          %.3f ns%n", avgNanos);
    }

    static int dictDecode(int[] values, int[] indices) {
        int sum = 0;
        for (int idx : indices) {
            sum += values[idx];
        }
        return sum;
    }

    static void benchmarkMutableDictInsert() {
        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            mutableDictInsert();
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            mutableDictInsert();
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("mutable_int_dict_insert:  %.3f µs%n", avgMicros);
    }

    static Map<Integer, Integer> mutableDictInsert() {
        Map<Integer, Integer> dict = new HashMap<>();
        for (int i = 0; i < DICT_SIZE; i++) {
            dict.putIfAbsent(i * 3, dict.size());
        }
        return dict;
    }

    // ============== Bitmap Benchmarks ==============

    static void benchmarkBitmapAnd() {
        BitSet bm1 = new BitSet();
        BitSet bm2 = new BitSet();
        Random rand = new Random(42);

        for (int i = 0; i < BITMAP_SIZE; i++) {
            bm1.set(rand.nextInt(10000));
            bm2.set(rand.nextInt(10000));
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            BitSet result = (BitSet) bm1.clone();
            result.and(bm2);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            BitSet result = (BitSet) bm1.clone();
            result.and(bm2);
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("bitmap_and:               %.3f µs%n", avgMicros);
    }

    static void benchmarkBitmapOr() {
        BitSet bm1 = new BitSet();
        BitSet bm2 = new BitSet();
        Random rand = new Random(42);

        for (int i = 0; i < BITMAP_SIZE; i++) {
            bm1.set(rand.nextInt(10000));
            bm2.set(rand.nextInt(10000));
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            BitSet result = (BitSet) bm1.clone();
            result.or(bm2);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            BitSet result = (BitSet) bm1.clone();
            result.or(bm2);
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("bitmap_or:                %.3f µs%n", avgMicros);
    }

    static void benchmarkBitmapIteration() {
        BitSet bm = new BitSet();
        Random rand = new Random(42);

        for (int i = 0; i < BITMAP_SIZE; i++) {
            bm.set(rand.nextInt(10000));
        }

        // Warmup
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            iterateBitmap(bm);
        }

        // Measure
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            iterateBitmap(bm);
            totalNanos += System.nanoTime() - start;
        }

        double avgMicros = (totalNanos / MEASURE_ITERATIONS) / 1000.0;
        System.out.printf("bitmap_iteration_5k:      %.3f µs%n", avgMicros);
    }

    static int iterateBitmap(BitSet bm) {
        int count = 0;
        for (int i = bm.nextSetBit(0); i >= 0; i = bm.nextSetBit(i + 1)) {
            count++;
        }
        return count;
    }
}

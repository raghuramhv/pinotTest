/**
 * Benchmark using RoaringBitmap (same as Rust uses)
 */
import org.roaringbitmap.RoaringBitmap;
import java.util.*;

public class RoaringBenchmark {
    static final int WARMUP_ITERATIONS = 5;
    static final int MEASURE_ITERATIONS = 10;
    static final int BITMAP_SIZE = 5_000;

    public static void main(String[] args) {
        System.out.println("=== Java RoaringBitmap Benchmark ===\n");

        RoaringBitmap bm1 = new RoaringBitmap();
        RoaringBitmap bm2 = new RoaringBitmap();
        Random rand = new Random(42);

        for (int i = 0; i < BITMAP_SIZE; i++) {
            bm1.add(rand.nextInt(10000));
            bm2.add(rand.nextInt(10000));
        }

        // AND benchmark
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            RoaringBitmap.and(bm1, bm2);
        }
        long totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            RoaringBitmap.and(bm1, bm2);
            totalNanos += System.nanoTime() - start;
        }
        System.out.printf("roaring_bitmap_and:       %.3f µs%n", (totalNanos / MEASURE_ITERATIONS) / 1000.0);

        // OR benchmark
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            RoaringBitmap.or(bm1, bm2);
        }
        totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            RoaringBitmap.or(bm1, bm2);
            totalNanos += System.nanoTime() - start;
        }
        System.out.printf("roaring_bitmap_or:        %.3f µs%n", (totalNanos / MEASURE_ITERATIONS) / 1000.0);

        // AND Cardinality
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            RoaringBitmap.andCardinality(bm1, bm2);
        }
        totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            RoaringBitmap.andCardinality(bm1, bm2);
            totalNanos += System.nanoTime() - start;
        }
        System.out.printf("roaring_and_cardinality:  %.3f µs%n", (totalNanos / MEASURE_ITERATIONS) / 1000.0);

        // OR Cardinality
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            RoaringBitmap.orCardinality(bm1, bm2);
        }
        totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            RoaringBitmap.orCardinality(bm1, bm2);
            totalNanos += System.nanoTime() - start;
        }
        System.out.printf("roaring_or_cardinality:   %.3f µs%n", (totalNanos / MEASURE_ITERATIONS) / 1000.0);

        // Iteration
        for (int i = 0; i < WARMUP_ITERATIONS; i++) {
            int count = 0;
            for (int v : bm1) count++;
        }
        totalNanos = 0;
        for (int i = 0; i < MEASURE_ITERATIONS; i++) {
            long start = System.nanoTime();
            int count = 0;
            for (int v : bm1) count++;
            totalNanos += System.nanoTime() - start;
        }
        System.out.printf("roaring_iteration:        %.3f µs%n", (totalNanos / MEASURE_ITERATIONS) / 1000.0);

        System.out.println("\n=== Complete ===");
    }
}

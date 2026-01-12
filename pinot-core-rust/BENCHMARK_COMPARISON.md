# Rust vs Java Performance Benchmark Comparison

## ⚠️ Run Benchmarks Yourself

**To verify these results, run the benchmarks on your own hardware.**

Results vary based on CPU, memory, OS, and JVM version. The numbers below are from a specific test run and should be reproduced before making engineering decisions.

---

## How to Run Benchmarks

### Step 1: Run Rust Benchmarks (Criterion)

```bash
cd pinot-core-rust

# Run all benchmarks
cargo bench

# Or run specific benchmark suites:
cargo bench --bench aggregation_bench
cargo bench --bench dictionary_bench
cargo bench --bench bitmap_bench
```

### Step 2: Run Java Benchmarks

```bash
cd pinot-core-rust/comparison_benchmark

# Compile the Java benchmark
javac JavaBenchmark.java

# Run with JVM optimizations
java -server -Xms512m -Xmx512m JavaBenchmark
```

For RoaringBitmap comparison (fairer than BitSet):
```bash
# Download RoaringBitmap
wget https://repo1.maven.org/maven2/org/roaringbitmap/RoaringBitmap/1.0.0/RoaringBitmap-1.0.0.jar

# Compile and run
javac -cp RoaringBitmap-1.0.0.jar RoaringBenchmark.java
java -cp .:RoaringBitmap-1.0.0.jar RoaringBenchmark
```

---

## Test Environment

| Property | Value |
|----------|-------|
| Platform | Linux 4.4.0 |
| Rust | Release build with LTO enabled |
| Java | OpenJDK with -Xms512m -Xmx512m |

---

## Sample Results

### Summary Table

| Benchmark | Rust | Java | Speedup |
|-----------|------|------|---------|
| **Aggregations** | | | |
| sum_10k | 11.8 µs | 95.7 µs | **8.1x faster** |
| count_10k | 0.35 ns | 73.4 ns | **210x faster** |
| min_10k | 6.1 µs | 78.3 µs | **12.8x faster** |
| max_10k | 6.1 µs | 70.3 µs | **11.5x faster** |
| avg_10k | 11.8 µs | 62.3 µs | **5.3x faster** |
| **Dictionaries** | | | |
| int_dict_lookup (1k ops) | 3.4 µs | 114.6 µs | **33.7x faster** |
| int_dict_decode (1k ops) | 314 ns | 15.3 µs | **48.7x faster** |
| mutable_dict_insert (1k) | 79.0 µs | 215.4 µs | **2.7x faster** |
| **Bitmaps** | | | |
| bitmap_and | 46.9 µs | 4.3 µs* | 0.09x (Java faster*) |
| bitmap_or | 41.6 µs | 3.7 µs* | 0.09x (Java faster*) |
| bitmap_iteration_5k | 18.5 µs | 140.3 µs | **7.6x faster** |

*Note: Java uses `BitSet` (simple bit array), Rust uses `RoaringBitmap` (compressed sparse bitmap). These are NOT equivalent data structures. BitSet is faster for dense bitmaps but uses significantly more memory.

### Rust Benchmark Results (criterion)

```
# Aggregations
sum_10k                  time:   [11.830 µs 11.835 µs 11.840 µs]
count_10k                time:   [352.30 ps 354.13 ps 356.25 ps]
min_max/min              time:   [6.0818 µs 6.1016 µs 6.1256 µs]
min_max/max              time:   [6.0491 µs 6.0673 µs 6.0892 µs]
avg_10k                  time:   [11.810 µs 11.813 µs 11.818 µs]
group_by_sum_100_groups  time:   [5.0679 µs 5.1075 µs 5.1547 µs]
executor_all_aggs        time:   [36.278 µs 36.299 µs 36.326 µs]

# Bitmaps (RoaringBitmap)
bitmap_iteration_5k      time:   [18.043 µs 18.545 µs 19.269 µs]
bitmap_and               time:   [46.712 µs 46.877 µs 47.057 µs]
bitmap_or                time:   [41.469 µs 41.626 µs 41.792 µs]
bitmap_not               time:   [13.056 µs 13.077 µs 13.104 µs]
cardinality/and          time:   [1.0175 µs 1.0205 µs 1.0237 µs]
cardinality/or           time:   [1.2143 µs 1.2241 µs 1.2356 µs]

# Dictionaries
int_dict_lookup          time:   [3.3343 µs 3.3530 µs 3.3801 µs]
int_dict_decode          time:   [311.88 ns 313.75 ns 315.92 ns]
string_dict_lookup       time:   [60.223 µs 61.880 µs 64.804 µs]
mutable_int_dict_insert  time:   [78.857 µs 79.015 µs 79.172 µs]
```

### Java Benchmark Results (JavaBenchmark.java)

```
# Aggregations
sum_10k:                  95.749 µs
count_10k:                73.400 ns
min_max/min:              78.348 µs
min_max/max:              70.345 µs
avg_10k:                  62.284 µs

# Dictionaries
int_dict_lookup:          114.568 µs
int_dict_decode:          15277.800 ns (15.3 µs)
mutable_int_dict_insert:  215.400 µs

# Bitmaps (BitSet - NOT comparable to RoaringBitmap)
bitmap_and:               4.277 µs
bitmap_or:                3.690 µs
bitmap_iteration_5k:      140.311 µs
```

---

## Analysis

### Where Rust Excels

1. **Aggregation Operations (5-12x faster)**
   - No boxing/unboxing overhead for primitive types
   - Better SIMD auto-vectorization in release builds
   - Zero-cost abstractions in Rust

2. **Dictionary Lookups (34-49x faster)**
   - `ahash` provides extremely fast hashing
   - No object header overhead (8-16 bytes per Java object)
   - Better cache locality with flat data structures

3. **Bitmap Iteration (7.6x faster)**
   - More efficient iterator implementation
   - No object allocation per iteration

### Important Caveats

1. **Bitmap AND/OR Operations**
   - Java BitSet is faster for dense bitmaps due to simpler implementation
   - RoaringBitmap (used in both Rust and Pinot) is optimized for sparse data
   - For fair comparison, use RoaringBitmap in both languages

2. **Simple Benchmarks vs Real Workloads**
   - These measure isolated operations
   - Real-world performance depends on memory pressure, GC, and access patterns

3. **JIT Compilation**
   - Java can approach Rust performance after warmup for simple operations
   - The Java benchmark uses simple timing, not JMH

4. **Memory Usage**
   - Rust: ~40 bytes per dictionary entry
   - Java: ~56-72 bytes per dictionary entry
   - Estimated 30-40% memory reduction with Rust

---

## Recording Your Results

Run benchmarks on your hardware and fill in:

### Your Test Environment

| Property | Value |
|----------|-------|
| Date | |
| CPU | |
| RAM | |
| OS | |
| Rust version | |
| Java version | |

### Your Results

| Benchmark | Rust (µs) | Java (µs) | Speedup |
|-----------|-----------|-----------|---------|
| sum_10k | | | |
| count_10k | | | |
| min_10k | | | |
| max_10k | | | |
| int_dict_lookup | | | |
| int_dict_decode | | | |
| bitmap_iteration | | | |

---

## Improving the Comparison

For more rigorous benchmarking:

1. **Use JMH for Java** - More accurate than simple System.nanoTime()
2. **Use same data structures** - Compare RoaringBitmap in both languages
3. **Test with realistic data** - Generate data matching your workload
4. **Consider GC impact** - Run longer tests to see GC effects
5. **Profile memory usage** - Compare heap sizes and allocation rates

---

## Questions?

If you have questions about the benchmark methodology:
1. Open an issue with your benchmark output
2. Include your test environment details
3. Describe any unexpected results

/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.
 */
package org.apache.pinot.benchmark;

import org.openjdk.jmh.annotations.*;
import org.openjdk.jmh.runner.Runner;
import org.openjdk.jmh.runner.options.Options;
import org.openjdk.jmh.runner.options.OptionsBuilder;
import org.openjdk.jmh.runner.options.TimeValue;

import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicLong;

/**
 * JMH Benchmark for Java broker operations.
 * Compares directly with Rust implementation benchmarks.
 */
@State(Scope.Benchmark)
@Fork(value = 1, jvmArgs = {"-Xms2G", "-Xmx2G"})
@Warmup(iterations = 3, time = 2)
@Measurement(iterations = 5, time = 3)
public class BrokerBenchmark {

    // ============== Routing Benchmark Data ==============

    @Param({"100", "1000", "10000"})
    private int numSegments;

    private List<String> segments;
    private List<ServerInstance> servers;
    private Map<String, List<ServerInstance>> segmentToServers;
    private RoutingTable routingTable;

    // ============== Reduce Benchmark Data ==============

    private List<DataTable> dataTables;
    private static final int ROWS_PER_TABLE = 10000;
    private static final int NUM_COLUMNS = 10;

    // ============== Aggregation Benchmark Data ==============

    @Param({"1000", "10000", "100000"})
    private int numGroups;

    private Map<String, double[]> groupData;

    @Setup(Level.Trial)
    public void setup() {
        setupRouting();
        setupReduce();
        setupAggregation();
    }

    private void setupRouting() {
        // Create segments
        segments = new ArrayList<>(numSegments);
        for (int i = 0; i < numSegments; i++) {
            segments.add("segment_" + i);
        }

        // Create servers (10 servers)
        servers = new ArrayList<>(10);
        for (int i = 0; i < 10; i++) {
            servers.add(new ServerInstance("server_" + i, "192.168.1." + i, 8099 + i));
        }

        // Create segment to server mapping (each segment on 2 random servers)
        segmentToServers = new HashMap<>();
        Random random = new Random(42);
        for (String segment : segments) {
            List<ServerInstance> segServers = new ArrayList<>();
            Set<Integer> used = new HashSet<>();
            while (segServers.size() < 2) {
                int idx = random.nextInt(servers.size());
                if (!used.contains(idx)) {
                    used.add(idx);
                    segServers.add(servers.get(idx));
                }
            }
            segmentToServers.put(segment, segServers);
        }

        // Create routing table
        routingTable = new RoutingTable(segmentToServers);
    }

    private void setupReduce() {
        // Create data tables to merge
        dataTables = new ArrayList<>(10);
        Random random = new Random(42);

        for (int t = 0; t < 10; t++) {
            List<Object[]> rows = new ArrayList<>(ROWS_PER_TABLE);
            for (int r = 0; r < ROWS_PER_TABLE; r++) {
                Object[] row = new Object[NUM_COLUMNS];
                for (int c = 0; c < NUM_COLUMNS; c++) {
                    if (c < 3) {
                        row[c] = "dim_" + random.nextInt(1000);
                    } else {
                        row[c] = random.nextDouble() * 1000;
                    }
                }
                rows.add(row);
            }
            dataTables.add(new DataTable(rows));
        }
    }

    private void setupAggregation() {
        // Create group data for aggregation
        groupData = new HashMap<>(numGroups);
        Random random = new Random(42);

        for (int g = 0; g < numGroups; g++) {
            String groupKey = "group_" + g;
            double[] values = new double[100]; // 100 values per group
            for (int v = 0; v < 100; v++) {
                values[v] = random.nextDouble() * 1000;
            }
            groupData.put(groupKey, values);
        }
    }

    // ============== Routing Benchmarks ==============

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public Map<ServerInstance, List<String>> balancedInstanceSelection() {
        return routingTable.selectServersBalanced(segments);
    }

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public Map<ServerInstance, List<String>> hashBasedInstanceSelection() {
        return routingTable.selectServersHashBased(segments);
    }

    // ============== Reduce Benchmarks ==============

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public List<Object[]> sequentialMerge() {
        List<Object[]> result = new ArrayList<>();
        for (DataTable table : dataTables) {
            result.addAll(table.getRows());
        }
        return result;
    }

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public List<Object[]> parallelMerge() {
        return dataTables.parallelStream()
                .flatMap(table -> table.getRows().stream())
                .collect(java.util.stream.Collectors.toList());
    }

    // ============== Aggregation Benchmarks ==============

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public Map<String, Double> sumAggregation() {
        Map<String, Double> result = new HashMap<>(numGroups);
        for (Map.Entry<String, double[]> entry : groupData.entrySet()) {
            double sum = 0;
            for (double v : entry.getValue()) {
                sum += v;
            }
            result.put(entry.getKey(), sum);
        }
        return result;
    }

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public Map<String, Double> parallelSumAggregation() {
        return groupData.entrySet().parallelStream()
                .collect(java.util.stream.Collectors.toMap(
                        Map.Entry::getKey,
                        e -> {
                            double sum = 0;
                            for (double v : e.getValue()) {
                                sum += v;
                            }
                            return sum;
                        }
                ));
    }

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public Map<String, double[]> minMaxAggregation() {
        Map<String, double[]> result = new HashMap<>(numGroups);
        for (Map.Entry<String, double[]> entry : groupData.entrySet()) {
            double min = Double.MAX_VALUE;
            double max = Double.MIN_VALUE;
            for (double v : entry.getValue()) {
                if (v < min) min = v;
                if (v > max) max = v;
            }
            result.put(entry.getKey(), new double[]{min, max});
        }
        return result;
    }

    // ============== Concurrent Operations Benchmarks ==============

    @Benchmark
    @BenchmarkMode(Mode.AverageTime)
    @OutputTimeUnit(TimeUnit.MICROSECONDS)
    public ConcurrentHashMap<String, AtomicLong> concurrentGroupBy() {
        ConcurrentHashMap<String, AtomicLong> result = new ConcurrentHashMap<>();
        dataTables.parallelStream()
                .flatMap(table -> table.getRows().stream())
                .forEach(row -> {
                    String key = (String) row[0];
                    result.computeIfAbsent(key, k -> new AtomicLong(0)).incrementAndGet();
                });
        return result;
    }

    // ============== Helper Classes ==============

    static class ServerInstance {
        final String name;
        final String host;
        final int port;

        ServerInstance(String name, String host, int port) {
            this.name = name;
            this.host = host;
            this.port = port;
        }

        @Override
        public int hashCode() {
            return name.hashCode();
        }

        @Override
        public boolean equals(Object o) {
            if (this == o) return true;
            if (!(o instanceof ServerInstance)) return false;
            return name.equals(((ServerInstance) o).name);
        }
    }

    static class DataTable {
        private final List<Object[]> rows;

        DataTable(List<Object[]> rows) {
            this.rows = rows;
        }

        List<Object[]> getRows() {
            return rows;
        }
    }

    static class RoutingTable {
        private final Map<String, List<ServerInstance>> segmentToServers;
        private final Map<ServerInstance, AtomicLong> serverLoads;

        RoutingTable(Map<String, List<ServerInstance>> segmentToServers) {
            this.segmentToServers = segmentToServers;
            this.serverLoads = new ConcurrentHashMap<>();
        }

        Map<ServerInstance, List<String>> selectServersBalanced(List<String> segments) {
            Map<ServerInstance, List<String>> result = new HashMap<>();
            Map<ServerInstance, Integer> currentLoad = new HashMap<>();

            for (String segment : segments) {
                List<ServerInstance> candidates = segmentToServers.get(segment);
                if (candidates == null || candidates.isEmpty()) continue;

                // Select server with minimum load
                ServerInstance selected = null;
                int minLoad = Integer.MAX_VALUE;
                for (ServerInstance server : candidates) {
                    int load = currentLoad.getOrDefault(server, 0);
                    if (load < minLoad) {
                        minLoad = load;
                        selected = server;
                    }
                }

                if (selected != null) {
                    result.computeIfAbsent(selected, k -> new ArrayList<>()).add(segment);
                    currentLoad.merge(selected, 1, Integer::sum);
                }
            }

            return result;
        }

        Map<ServerInstance, List<String>> selectServersHashBased(List<String> segments) {
            Map<ServerInstance, List<String>> result = new HashMap<>();

            for (String segment : segments) {
                List<ServerInstance> candidates = segmentToServers.get(segment);
                if (candidates == null || candidates.isEmpty()) continue;

                // Select server based on hash
                int hash = segment.hashCode();
                ServerInstance selected = candidates.get(Math.abs(hash) % candidates.size());

                result.computeIfAbsent(selected, k -> new ArrayList<>()).add(segment);
            }

            return result;
        }
    }

    public static void main(String[] args) throws Exception {
        Options opt = new OptionsBuilder()
                .include(BrokerBenchmark.class.getSimpleName())
                .warmupIterations(3)
                .warmupTime(TimeValue.seconds(2))
                .measurementIterations(5)
                .measurementTime(TimeValue.seconds(3))
                .forks(1)
                .build();

        new Runner(opt).run();
    }
}

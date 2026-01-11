/**
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *   http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */
package org.apache.pinot.core.native;

import org.testng.annotations.BeforeClass;
import org.testng.annotations.Test;

import static org.testng.Assert.*;


/**
 * Integration tests for Native (Rust) library functionality.
 * These tests verify that the JNI bridge works correctly when the native library is available.
 * Tests are skipped if the native library is not available.
 */
public class NativeIntegrationTest {

  private boolean _nativeAvailable;

  @BeforeClass
  public void setUp() {
    _nativeAvailable = NativeLibraryLoader.isAvailable();
    if (!_nativeAvailable) {
      System.out.println("Native library not available - tests will verify fallback behavior");
    } else {
      System.out.println("Native library loaded successfully - testing native functions");
    }
  }

  // ==================== Aggregation Tests ====================

  @Test
  public void testNativeAggregationAvailability() {
    // Just verify we can check availability without crashing
    boolean available = NativeAggregation.isAvailable();
    assertEquals(available, _nativeAvailable);
  }

  @Test
  public void testIntSumAggregation() {
    int[] values = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10};
    long result = NativeAggregation.sumInt(values);
    assertEquals(result, 55L);
  }

  @Test
  public void testLongSumAggregation() {
    long[] values = {100L, 200L, 300L, 400L, 500L};
    long result = NativeAggregation.sumLong(values);
    assertEquals(result, 1500L);
  }

  @Test
  public void testDoubleSumAggregation() {
    double[] values = {1.5, 2.5, 3.5, 4.5, 5.5};
    double result = NativeAggregation.sumDouble(values);
    assertEquals(result, 17.5, 0.0001);
  }

  @Test
  public void testIntMinMax() {
    int[] values = {5, 2, 9, 1, 7, 3, 8, 4, 6};
    assertEquals(NativeAggregation.minInt(values), 1);
    assertEquals(NativeAggregation.maxInt(values), 9);
  }

  @Test
  public void testLongMinMax() {
    long[] values = {500L, 200L, 900L, 100L, 700L};
    assertEquals(NativeAggregation.minLong(values), 100L);
    assertEquals(NativeAggregation.maxLong(values), 900L);
  }

  @Test
  public void testDoubleMinMax() {
    double[] values = {5.5, 2.2, 9.9, 1.1, 7.7};
    assertEquals(NativeAggregation.minDouble(values), 1.1, 0.0001);
    assertEquals(NativeAggregation.maxDouble(values), 9.9, 0.0001);
  }

  @Test
  public void testCount() {
    int[] values = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10};
    assertEquals(NativeAggregation.count(values), 10L);
  }

  @Test
  public void testAverage() {
    double[] values = {2.0, 4.0, 6.0, 8.0, 10.0};
    assertEquals(NativeAggregation.avgDouble(values), 6.0, 0.0001);
  }

  @Test
  public void testEmptyArray() {
    int[] empty = {};
    assertEquals(NativeAggregation.sumInt(empty), 0L);
    assertEquals(NativeAggregation.count(empty), 0L);
  }

  // ==================== Dictionary Tests ====================

  @Test
  public void testIntDictionary() {
    if (!NativeDictionary.isAvailable()) {
      System.out.println("Skipping int dictionary test - native not available");
      return;
    }

    int[] sortedValues = {10, 20, 30, 40, 50};
    try (NativeDictionary.IntDictionaryHandle dict = NativeDictionary.createIntDictionary(sortedValues)) {
      assertNotNull(dict);
      assertEquals(dict.size(), 5);

      // Test indexOf
      assertEquals(dict.indexOf(10), 0);
      assertEquals(dict.indexOf(30), 2);
      assertEquals(dict.indexOf(50), 4);
      assertEquals(dict.indexOf(25), -1); // Not found

      // Test get
      assertEquals(dict.get(0), 10);
      assertEquals(dict.get(2), 30);
      assertEquals(dict.get(4), 50);

      // Test batch decode
      int[] dictIds = {0, 2, 4};
      int[] output = new int[3];
      dict.batchDecode(dictIds, output);
      assertEquals(output[0], 10);
      assertEquals(output[1], 30);
      assertEquals(output[2], 50);
    }
  }

  @Test
  public void testLongDictionary() {
    if (!NativeDictionary.isAvailable()) {
      System.out.println("Skipping long dictionary test - native not available");
      return;
    }

    long[] sortedValues = {100L, 200L, 300L, 400L, 500L};
    try (NativeDictionary.LongDictionaryHandle dict = NativeDictionary.createLongDictionary(sortedValues)) {
      assertNotNull(dict);
      assertEquals(dict.size(), 5);

      // Test indexOf
      assertEquals(dict.indexOf(100L), 0);
      assertEquals(dict.indexOf(300L), 2);
      assertEquals(dict.indexOf(250L), -1); // Not found

      // Test get
      assertEquals(dict.get(0), 100L);
      assertEquals(dict.get(4), 500L);

      // Test batch decode
      int[] dictIds = {1, 3};
      long[] output = new long[2];
      dict.batchDecode(dictIds, output);
      assertEquals(output[0], 200L);
      assertEquals(output[1], 400L);
    }
  }

  @Test
  public void testStringDictionary() {
    if (!NativeDictionary.isAvailable()) {
      System.out.println("Skipping string dictionary test - native not available");
      return;
    }

    String[] sortedValues = {"apple", "banana", "cherry", "date", "elderberry"};
    try (NativeDictionary.StringDictionaryHandle dict = NativeDictionary.createStringDictionary(sortedValues)) {
      assertNotNull(dict);
      assertEquals(dict.size(), 5);

      // Test indexOf
      assertEquals(dict.indexOf("apple"), 0);
      assertEquals(dict.indexOf("cherry"), 2);
      assertEquals(dict.indexOf("fig"), -1); // Not found

      // Test get
      assertEquals(dict.get(0), "apple");
      assertEquals(dict.get(2), "cherry");
      assertEquals(dict.get(4), "elderberry");
    }
  }

  // ==================== Bitmap Tests ====================

  @Test
  public void testBitmapAvailability() {
    boolean available = NativeBitmap.isAvailable();
    assertEquals(available, _nativeAvailable);
  }

  @Test
  public void testBitmapAndCardinality() {
    if (!NativeBitmap.isAvailable()) {
      System.out.println("Skipping bitmap AND test - native not available");
      return;
    }

    org.roaringbitmap.RoaringBitmap bitmap1 = new org.roaringbitmap.RoaringBitmap();
    bitmap1.add(1, 2, 3, 4, 5);

    org.roaringbitmap.RoaringBitmap bitmap2 = new org.roaringbitmap.RoaringBitmap();
    bitmap2.add(3, 4, 5, 6, 7);

    byte[] serialized1 = serializeBitmap(bitmap1);
    byte[] serialized2 = serializeBitmap(bitmap2);

    long andCardinality = NativeBitmap.andCardinality(serialized1, serialized2);
    assertEquals(andCardinality, 3L); // 3, 4, 5 are common
  }

  @Test
  public void testBitmapOrCardinality() {
    if (!NativeBitmap.isAvailable()) {
      System.out.println("Skipping bitmap OR test - native not available");
      return;
    }

    org.roaringbitmap.RoaringBitmap bitmap1 = new org.roaringbitmap.RoaringBitmap();
    bitmap1.add(1, 2, 3);

    org.roaringbitmap.RoaringBitmap bitmap2 = new org.roaringbitmap.RoaringBitmap();
    bitmap2.add(3, 4, 5);

    byte[] serialized1 = serializeBitmap(bitmap1);
    byte[] serialized2 = serializeBitmap(bitmap2);

    long orCardinality = NativeBitmap.orCardinality(serialized1, serialized2);
    assertEquals(orCardinality, 5L); // 1, 2, 3, 4, 5
  }

  @Test
  public void testBitmapXorCardinality() {
    if (!NativeBitmap.isAvailable()) {
      System.out.println("Skipping bitmap XOR test - native not available");
      return;
    }

    org.roaringbitmap.RoaringBitmap bitmap1 = new org.roaringbitmap.RoaringBitmap();
    bitmap1.add(1, 2, 3);

    org.roaringbitmap.RoaringBitmap bitmap2 = new org.roaringbitmap.RoaringBitmap();
    bitmap2.add(2, 3, 4);

    byte[] serialized1 = serializeBitmap(bitmap1);
    byte[] serialized2 = serializeBitmap(bitmap2);

    long xorCardinality = NativeBitmap.xorCardinality(serialized1, serialized2);
    assertEquals(xorCardinality, 2L); // 1 and 4 (exclusive to one or the other)
  }

  // ==================== Configuration Tests ====================

  @Test
  public void testNativeConfig() {
    // Test configuration methods don't throw
    NativeConfig.reset(); // Reset to default state

    boolean nativeEnabled = NativeConfig.isNativeEnabled();
    boolean aggregationEnabled = NativeConfig.isAggregationEnabled();
    boolean dictionaryEnabled = NativeConfig.isDictionaryEnabled();
    boolean bitmapEnabled = NativeConfig.isBitmapEnabled();
    int minBatchSize = NativeConfig.getMinBatchSize();

    // Verify defaults when native is available
    if (_nativeAvailable) {
      assertTrue(nativeEnabled);
      assertTrue(aggregationEnabled);
      assertTrue(dictionaryEnabled);
      assertTrue(bitmapEnabled);
    }

    assertEquals(minBatchSize, 1000);
  }

  @Test
  public void testShouldUseNative() {
    NativeConfig.reset();

    // Small batch - should not use native
    assertFalse(NativeConfig.shouldUseNative(500));

    // Large batch - should use native if available
    assertEquals(NativeConfig.shouldUseNative(2000), _nativeAvailable);
  }

  // ==================== Helper Methods ====================

  private byte[] serializeBitmap(org.roaringbitmap.RoaringBitmap bitmap) {
    try {
      java.io.ByteArrayOutputStream baos = new java.io.ByteArrayOutputStream();
      java.io.DataOutputStream dos = new java.io.DataOutputStream(baos);
      bitmap.serialize(dos);
      return baos.toByteArray();
    } catch (java.io.IOException e) {
      throw new RuntimeException("Failed to serialize bitmap", e);
    }
  }
}

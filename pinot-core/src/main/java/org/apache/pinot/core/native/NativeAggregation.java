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

import org.roaringbitmap.RoaringBitmap;


/**
 * Native (Rust) implementations of aggregation functions.
 * These provide significant performance improvements over pure Java implementations
 * for large datasets due to better SIMD utilization and reduced overhead.
 *
 * <p>All methods gracefully fall back to Java implementations if native library
 * is not available.</p>
 */
public final class NativeAggregation {

  private static final boolean NATIVE_AVAILABLE = NativeLibraryLoader.load();

  private NativeAggregation() {
  }

  /**
   * Returns true if native aggregation functions are available.
   */
  public static boolean isAvailable() {
    return NATIVE_AVAILABLE;
  }

  // ==================== SUM ====================

  /**
   * Computes the sum of a double array using native SIMD instructions.
   *
   * @param values the values to sum
   * @return the sum
   */
  public static double sumDouble(double[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeSumDoubleArray(values);
    }
    return sumDoubleJava(values);
  }

  /**
   * Computes the sum of a double array with null handling.
   *
   * @param values the values to sum
   * @param nullBitmap bitmap where set bits indicate null values (can be null)
   * @return the sum, or null if all values are null
   */
  public static Double sumDoubleWithNulls(double[] values, RoaringBitmap nullBitmap) {
    if (NATIVE_AVAILABLE && values.length > 0 && nullBitmap != null) {
      long[] nullBitmapArray = nullBitmap.toArray().length > 0
          ? toLongArray(nullBitmap.toArray())
          : null;
      return nativeSumDoubleArrayWithNulls(values, nullBitmapArray);
    }
    return sumDoubleWithNullsJava(values, nullBitmap);
  }

  /**
   * Computes the sum of an int array.
   */
  public static long sumInt(int[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeSumIntArray(values);
    }
    return sumIntJava(values);
  }

  /**
   * Computes the sum of a long array.
   */
  public static long sumLong(long[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeSumLongArray(values);
    }
    return sumLongJava(values);
  }

  // ==================== COUNT ====================

  /**
   * Counts elements, excluding nulls if bitmap provided.
   */
  public static long count(int length, RoaringBitmap nullBitmap) {
    if (nullBitmap == null || nullBitmap.isEmpty()) {
      return length;
    }
    return length - nullBitmap.getCardinality();
  }

  // ==================== MIN ====================

  /**
   * Finds the minimum value in a double array.
   */
  public static double minDouble(double[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeMinDoubleArray(values);
    }
    return minDoubleJava(values);
  }

  /**
   * Finds the minimum value in a double array with null handling.
   */
  public static Double minDoubleWithNulls(double[] values, RoaringBitmap nullBitmap) {
    if (NATIVE_AVAILABLE && values.length > 0 && nullBitmap != null) {
      long[] nullBitmapArray = nullBitmap.toArray().length > 0
          ? toLongArray(nullBitmap.toArray())
          : null;
      return nativeMinDoubleArrayWithNulls(values, nullBitmapArray);
    }
    return minDoubleWithNullsJava(values, nullBitmap);
  }

  /**
   * Finds the minimum value in a long array.
   */
  public static long minLong(long[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeMinLongArray(values);
    }
    return minLongJava(values);
  }

  // ==================== MAX ====================

  /**
   * Finds the maximum value in a double array.
   */
  public static double maxDouble(double[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeMaxDoubleArray(values);
    }
    return maxDoubleJava(values);
  }

  /**
   * Finds the maximum value in a double array with null handling.
   */
  public static Double maxDoubleWithNulls(double[] values, RoaringBitmap nullBitmap) {
    if (NATIVE_AVAILABLE && values.length > 0 && nullBitmap != null) {
      long[] nullBitmapArray = nullBitmap.toArray().length > 0
          ? toLongArray(nullBitmap.toArray())
          : null;
      return nativeMaxDoubleArrayWithNulls(values, nullBitmapArray);
    }
    return maxDoubleWithNullsJava(values, nullBitmap);
  }

  /**
   * Finds the maximum value in a long array.
   */
  public static long maxLong(long[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeMaxLongArray(values);
    }
    return maxLongJava(values);
  }

  // ==================== AVG ====================

  /**
   * Computes the average of a double array.
   *
   * @param values the values to average
   * @return array of [sum, count]
   */
  public static double[] avgDouble(double[] values) {
    if (NATIVE_AVAILABLE && values.length > 0) {
      return nativeAvgDoubleArray(values);
    }
    return avgDoubleJava(values);
  }

  // ==================== Native Method Declarations ====================

  private static native double nativeSumDoubleArray(double[] values);
  private static native Double nativeSumDoubleArrayWithNulls(double[] values, long[] nullBitmap);
  private static native long nativeSumIntArray(int[] values);
  private static native long nativeSumLongArray(long[] values);

  private static native double nativeMinDoubleArray(double[] values);
  private static native Double nativeMinDoubleArrayWithNulls(double[] values, long[] nullBitmap);
  private static native long nativeMinLongArray(long[] values);

  private static native double nativeMaxDoubleArray(double[] values);
  private static native Double nativeMaxDoubleArrayWithNulls(double[] values, long[] nullBitmap);
  private static native long nativeMaxLongArray(long[] values);

  private static native double[] nativeAvgDoubleArray(double[] values);

  // ==================== Java Fallback Implementations ====================

  private static double sumDoubleJava(double[] values) {
    double sum = 0;
    for (double v : values) {
      sum += v;
    }
    return sum;
  }

  private static Double sumDoubleWithNullsJava(double[] values, RoaringBitmap nullBitmap) {
    if (nullBitmap != null && nullBitmap.getCardinality() == values.length) {
      return null;
    }
    double sum = 0;
    boolean hasValue = false;
    for (int i = 0; i < values.length; i++) {
      if (nullBitmap == null || !nullBitmap.contains(i)) {
        sum += values[i];
        hasValue = true;
      }
    }
    return hasValue ? sum : null;
  }

  private static long sumIntJava(int[] values) {
    long sum = 0;
    for (int v : values) {
      sum += v;
    }
    return sum;
  }

  private static long sumLongJava(long[] values) {
    long sum = 0;
    for (long v : values) {
      sum += v;
    }
    return sum;
  }

  private static double minDoubleJava(double[] values) {
    double min = Double.MAX_VALUE;
    for (double v : values) {
      if (v < min) {
        min = v;
      }
    }
    return min;
  }

  private static Double minDoubleWithNullsJava(double[] values, RoaringBitmap nullBitmap) {
    Double min = null;
    for (int i = 0; i < values.length; i++) {
      if (nullBitmap == null || !nullBitmap.contains(i)) {
        if (min == null || values[i] < min) {
          min = values[i];
        }
      }
    }
    return min;
  }

  private static long minLongJava(long[] values) {
    long min = Long.MAX_VALUE;
    for (long v : values) {
      if (v < min) {
        min = v;
      }
    }
    return min;
  }

  private static double maxDoubleJava(double[] values) {
    double max = Double.MIN_VALUE;
    for (double v : values) {
      if (v > max) {
        max = v;
      }
    }
    return max;
  }

  private static Double maxDoubleWithNullsJava(double[] values, RoaringBitmap nullBitmap) {
    Double max = null;
    for (int i = 0; i < values.length; i++) {
      if (nullBitmap == null || !nullBitmap.contains(i)) {
        if (max == null || values[i] > max) {
          max = values[i];
        }
      }
    }
    return max;
  }

  private static long maxLongJava(long[] values) {
    long max = Long.MIN_VALUE;
    for (long v : values) {
      if (v > max) {
        max = v;
      }
    }
    return max;
  }

  private static double[] avgDoubleJava(double[] values) {
    double sum = 0;
    for (double v : values) {
      sum += v;
    }
    return new double[]{sum, values.length};
  }

  private static long[] toLongArray(int[] intArray) {
    long[] result = new long[intArray.length];
    for (int i = 0; i < intArray.length; i++) {
      result[i] = intArray[i];
    }
    return result;
  }
}

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

import java.nio.ByteBuffer;
import org.roaringbitmap.RoaringBitmap;
import org.roaringbitmap.buffer.ImmutableRoaringBitmap;


/**
 * Native (Rust) bitmap operations for high-performance filter evaluation.
 * Provides fast cardinality computation and set operations on serialized bitmaps.
 */
public final class NativeBitmap {

  private static final boolean NATIVE_AVAILABLE = NativeLibraryLoader.load();

  private NativeBitmap() {
  }

  /**
   * Returns true if native bitmap functions are available.
   */
  public static boolean isAvailable() {
    return NATIVE_AVAILABLE;
  }

  // ==================== Cardinality Operations ====================

  /**
   * Computes AND cardinality of two bitmaps without materializing the result.
   * This is significantly faster than creating the AND bitmap and getting cardinality.
   *
   * @param bitmap1 serialized bitmap 1
   * @param bitmap2 serialized bitmap 2
   * @return the cardinality of bitmap1 AND bitmap2
   */
  public static long andCardinality(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeAndCardinality(bitmap1, bitmap2);
    }
    return andCardinalityJava(bitmap1, bitmap2);
  }

  /**
   * Computes OR cardinality of two bitmaps without materializing the result.
   */
  public static long orCardinality(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeOrCardinality(bitmap1, bitmap2);
    }
    return orCardinalityJava(bitmap1, bitmap2);
  }

  /**
   * Computes AND-NOT cardinality (bitmap1 AND NOT bitmap2).
   */
  public static long andNotCardinality(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeAndNotCardinality(bitmap1, bitmap2);
    }
    return andNotCardinalityJava(bitmap1, bitmap2);
  }

  /**
   * Computes XOR cardinality of two bitmaps.
   */
  public static long xorCardinality(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeXorCardinality(bitmap1, bitmap2);
    }
    return xorCardinalityJava(bitmap1, bitmap2);
  }

  // ==================== Convenience Methods for RoaringBitmap ====================

  /**
   * Computes AND cardinality using RoaringBitmap objects.
   */
  public static long andCardinality(RoaringBitmap bitmap1, RoaringBitmap bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeAndCardinality(bitmap1.serialize(), bitmap2.serialize());
    }
    return RoaringBitmap.andCardinality(bitmap1, bitmap2);
  }

  /**
   * Computes OR cardinality using RoaringBitmap objects.
   */
  public static long orCardinality(RoaringBitmap bitmap1, RoaringBitmap bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeOrCardinality(bitmap1.serialize(), bitmap2.serialize());
    }
    return RoaringBitmap.orCardinality(bitmap1, bitmap2);
  }

  /**
   * Computes AND-NOT cardinality using RoaringBitmap objects.
   */
  public static long andNotCardinality(RoaringBitmap bitmap1, RoaringBitmap bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeAndNotCardinality(bitmap1.serialize(), bitmap2.serialize());
    }
    return RoaringBitmap.andNotCardinality(bitmap1, bitmap2);
  }

  // ==================== Set Operations (returning serialized result) ====================

  /**
   * Computes AND of two bitmaps, returning serialized result.
   */
  public static byte[] and(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeAnd(bitmap1, bitmap2);
    }
    return andJava(bitmap1, bitmap2);
  }

  /**
   * Computes OR of two bitmaps, returning serialized result.
   */
  public static byte[] or(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeOr(bitmap1, bitmap2);
    }
    return orJava(bitmap1, bitmap2);
  }

  /**
   * Computes AND-NOT of two bitmaps, returning serialized result.
   */
  public static byte[] andNot(byte[] bitmap1, byte[] bitmap2) {
    if (NATIVE_AVAILABLE) {
      return nativeAndNot(bitmap1, bitmap2);
    }
    return andNotJava(bitmap1, bitmap2);
  }

  // ==================== Native Method Declarations ====================

  private static native long nativeAndCardinality(byte[] bitmap1, byte[] bitmap2);
  private static native long nativeOrCardinality(byte[] bitmap1, byte[] bitmap2);
  private static native long nativeAndNotCardinality(byte[] bitmap1, byte[] bitmap2);
  private static native long nativeXorCardinality(byte[] bitmap1, byte[] bitmap2);

  private static native byte[] nativeAnd(byte[] bitmap1, byte[] bitmap2);
  private static native byte[] nativeOr(byte[] bitmap1, byte[] bitmap2);
  private static native byte[] nativeAndNot(byte[] bitmap1, byte[] bitmap2);

  // ==================== Java Fallback Implementations ====================

  private static long andCardinalityJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      ImmutableRoaringBitmap bm1 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap1));
      ImmutableRoaringBitmap bm2 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap2));
      return ImmutableRoaringBitmap.andCardinality(bm1, bm2);
    } catch (Exception e) {
      return 0;
    }
  }

  private static long orCardinalityJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      ImmutableRoaringBitmap bm1 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap1));
      ImmutableRoaringBitmap bm2 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap2));
      return ImmutableRoaringBitmap.orCardinality(bm1, bm2);
    } catch (Exception e) {
      return 0;
    }
  }

  private static long andNotCardinalityJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      ImmutableRoaringBitmap bm1 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap1));
      ImmutableRoaringBitmap bm2 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap2));
      return ImmutableRoaringBitmap.andNotCardinality(bm1, bm2);
    } catch (Exception e) {
      return 0;
    }
  }

  private static long xorCardinalityJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      ImmutableRoaringBitmap bm1 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap1));
      ImmutableRoaringBitmap bm2 = new ImmutableRoaringBitmap(ByteBuffer.wrap(bitmap2));
      return ImmutableRoaringBitmap.xorCardinality(bm1, bm2);
    } catch (Exception e) {
      return 0;
    }
  }

  private static byte[] andJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      RoaringBitmap bm1 = new RoaringBitmap();
      bm1.deserialize(ByteBuffer.wrap(bitmap1));
      RoaringBitmap bm2 = new RoaringBitmap();
      bm2.deserialize(ByteBuffer.wrap(bitmap2));
      RoaringBitmap result = RoaringBitmap.and(bm1, bm2);
      return result.serialize();
    } catch (Exception e) {
      return new byte[0];
    }
  }

  private static byte[] orJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      RoaringBitmap bm1 = new RoaringBitmap();
      bm1.deserialize(ByteBuffer.wrap(bitmap1));
      RoaringBitmap bm2 = new RoaringBitmap();
      bm2.deserialize(ByteBuffer.wrap(bitmap2));
      RoaringBitmap result = RoaringBitmap.or(bm1, bm2);
      return result.serialize();
    } catch (Exception e) {
      return new byte[0];
    }
  }

  private static byte[] andNotJava(byte[] bitmap1, byte[] bitmap2) {
    try {
      RoaringBitmap bm1 = new RoaringBitmap();
      bm1.deserialize(ByteBuffer.wrap(bitmap1));
      RoaringBitmap bm2 = new RoaringBitmap();
      bm2.deserialize(ByteBuffer.wrap(bitmap2));
      RoaringBitmap result = RoaringBitmap.andNot(bm1, bm2);
      return result.serialize();
    } catch (Exception e) {
      return new byte[0];
    }
  }
}

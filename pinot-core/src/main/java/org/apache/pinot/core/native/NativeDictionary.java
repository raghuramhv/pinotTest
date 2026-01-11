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

import java.io.Closeable;


/**
 * Native (Rust) dictionary implementations for high-performance encoding/decoding.
 * These provide significant speedup over Java HashMap-based dictionaries.
 *
 * <p>Dictionary handles are native pointers that must be closed when no longer needed
 * to avoid memory leaks.</p>
 */
public final class NativeDictionary {

  private static final boolean NATIVE_AVAILABLE = NativeLibraryLoader.load();

  private NativeDictionary() {
  }

  /**
   * Returns true if native dictionary functions are available.
   */
  public static boolean isAvailable() {
    return NATIVE_AVAILABLE;
  }

  // ==================== Int Dictionary ====================

  /**
   * Creates a native int dictionary from sorted values.
   *
   * @param sortedValues the sorted int values
   * @return a handle to the native dictionary, or null if native not available
   */
  public static IntDictionaryHandle createIntDictionary(int[] sortedValues) {
    if (!NATIVE_AVAILABLE) {
      return null;
    }
    long handle = nativeCreateIntDictionary(sortedValues);
    return new IntDictionaryHandle(handle);
  }

  /**
   * Handle to a native int dictionary. Must be closed when no longer needed.
   */
  public static class IntDictionaryHandle implements Closeable {
    private long _handle;
    private volatile boolean _closed = false;

    private IntDictionaryHandle(long handle) {
      _handle = handle;
    }

    /**
     * Looks up the dictionary ID for a value.
     *
     * @param value the value to look up
     * @return the dictionary ID, or -1 if not found
     */
    public int indexOf(int value) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeIntDictionaryIndexOf(_handle, value);
    }

    /**
     * Gets the value for a dictionary ID.
     *
     * @param dictId the dictionary ID
     * @return the value
     */
    public int get(int dictId) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeIntDictionaryGet(_handle, dictId);
    }

    /**
     * Batch decode dictionary IDs to values.
     *
     * @param dictIds the dictionary IDs to decode
     * @param output the output array (must be same length as dictIds)
     */
    public void batchDecode(int[] dictIds, int[] output) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      nativeIntDictionaryBatchDecode(_handle, dictIds, output);
    }

    /**
     * Returns the dictionary size.
     */
    public int size() {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeIntDictionarySize(_handle);
    }

    @Override
    public void close() {
      if (!_closed) {
        _closed = true;
        nativeFreeIntDictionary(_handle);
        _handle = 0;
      }
    }

    @Override
    protected void finalize() throws Throwable {
      try {
        close();
      } finally {
        super.finalize();
      }
    }
  }

  // ==================== Long Dictionary ====================

  /**
   * Creates a native long dictionary from sorted values.
   */
  public static LongDictionaryHandle createLongDictionary(long[] sortedValues) {
    if (!NATIVE_AVAILABLE) {
      return null;
    }
    long handle = nativeCreateLongDictionary(sortedValues);
    return new LongDictionaryHandle(handle);
  }

  /**
   * Handle to a native long dictionary.
   */
  public static class LongDictionaryHandle implements Closeable {
    private long _handle;
    private volatile boolean _closed = false;

    private LongDictionaryHandle(long handle) {
      _handle = handle;
    }

    public int indexOf(long value) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeLongDictionaryIndexOf(_handle, value);
    }

    public long get(int dictId) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeLongDictionaryGet(_handle, dictId);
    }

    public void batchDecode(int[] dictIds, long[] output) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      nativeLongDictionaryBatchDecode(_handle, dictIds, output);
    }

    public int size() {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeLongDictionarySize(_handle);
    }

    @Override
    public void close() {
      if (!_closed) {
        _closed = true;
        nativeFreeLongDictionary(_handle);
        _handle = 0;
      }
    }

    @Override
    protected void finalize() throws Throwable {
      try {
        close();
      } finally {
        super.finalize();
      }
    }
  }

  // ==================== String Dictionary ====================

  /**
   * Creates a native string dictionary from sorted values.
   */
  public static StringDictionaryHandle createStringDictionary(String[] sortedValues) {
    if (!NATIVE_AVAILABLE) {
      return null;
    }
    long handle = nativeCreateStringDictionary(sortedValues);
    return new StringDictionaryHandle(handle);
  }

  /**
   * Handle to a native string dictionary.
   */
  public static class StringDictionaryHandle implements Closeable {
    private long _handle;
    private volatile boolean _closed = false;

    private StringDictionaryHandle(long handle) {
      _handle = handle;
    }

    public int indexOf(String value) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeStringDictionaryIndexOf(_handle, value);
    }

    public String get(int dictId) {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeStringDictionaryGet(_handle, dictId);
    }

    public int size() {
      if (_closed) {
        throw new IllegalStateException("Dictionary has been closed");
      }
      return nativeStringDictionarySize(_handle);
    }

    @Override
    public void close() {
      if (!_closed) {
        _closed = true;
        nativeFreeStringDictionary(_handle);
        _handle = 0;
      }
    }

    @Override
    protected void finalize() throws Throwable {
      try {
        close();
      } finally {
        super.finalize();
      }
    }
  }

  // ==================== Native Method Declarations ====================

  // Int dictionary
  private static native long nativeCreateIntDictionary(int[] sortedValues);
  private static native int nativeIntDictionaryIndexOf(long handle, int value);
  private static native int nativeIntDictionaryGet(long handle, int dictId);
  private static native void nativeIntDictionaryBatchDecode(long handle, int[] dictIds, int[] output);
  private static native int nativeIntDictionarySize(long handle);
  private static native void nativeFreeIntDictionary(long handle);

  // Long dictionary
  private static native long nativeCreateLongDictionary(long[] sortedValues);
  private static native int nativeLongDictionaryIndexOf(long handle, long value);
  private static native long nativeLongDictionaryGet(long handle, int dictId);
  private static native void nativeLongDictionaryBatchDecode(long handle, int[] dictIds, long[] output);
  private static native int nativeLongDictionarySize(long handle);
  private static native void nativeFreeLongDictionary(long handle);

  // String dictionary
  private static native long nativeCreateStringDictionary(String[] sortedValues);
  private static native int nativeStringDictionaryIndexOf(long handle, String value);
  private static native String nativeStringDictionaryGet(long handle, int dictId);
  private static native int nativeStringDictionarySize(long handle);
  private static native void nativeFreeStringDictionary(long handle);
}

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

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;


/**
 * Configuration for native (Rust) optimizations in Pinot.
 * These settings can be controlled via system properties or environment variables.
 *
 * <p>System properties take precedence over environment variables.</p>
 *
 * <h3>Configuration Options:</h3>
 * <ul>
 *   <li>{@code pinot.native.enabled} - Master switch to enable/disable all native optimizations (default: true)</li>
 *   <li>{@code pinot.native.aggregation.enabled} - Enable native aggregation functions (default: true)</li>
 *   <li>{@code pinot.native.dictionary.enabled} - Enable native dictionary operations (default: true)</li>
 *   <li>{@code pinot.native.bitmap.enabled} - Enable native bitmap operations (default: true)</li>
 *   <li>{@code pinot.native.min.batch.size} - Minimum batch size to use native (default: 1000)</li>
 * </ul>
 *
 * <h3>Example Usage:</h3>
 * <pre>
 * # Enable all native optimizations (default)
 * java -Dpinot.native.enabled=true ...
 *
 * # Disable native optimizations entirely
 * java -Dpinot.native.enabled=false ...
 *
 * # Enable only aggregation, disable dictionary and bitmap
 * java -Dpinot.native.enabled=true \
 *      -Dpinot.native.aggregation.enabled=true \
 *      -Dpinot.native.dictionary.enabled=false \
 *      -Dpinot.native.bitmap.enabled=false ...
 * </pre>
 */
public final class NativeConfig {
  private static final Logger LOGGER = LoggerFactory.getLogger(NativeConfig.class);

  // System property names
  public static final String NATIVE_ENABLED = "pinot.native.enabled";
  public static final String NATIVE_AGGREGATION_ENABLED = "pinot.native.aggregation.enabled";
  public static final String NATIVE_DICTIONARY_ENABLED = "pinot.native.dictionary.enabled";
  public static final String NATIVE_BITMAP_ENABLED = "pinot.native.bitmap.enabled";
  public static final String NATIVE_MIN_BATCH_SIZE = "pinot.native.min.batch.size";

  // Environment variable names (alternative to system properties)
  public static final String ENV_NATIVE_ENABLED = "PINOT_NATIVE_ENABLED";
  public static final String ENV_NATIVE_AGGREGATION_ENABLED = "PINOT_NATIVE_AGGREGATION_ENABLED";
  public static final String ENV_NATIVE_DICTIONARY_ENABLED = "PINOT_NATIVE_DICTIONARY_ENABLED";
  public static final String ENV_NATIVE_BITMAP_ENABLED = "PINOT_NATIVE_BITMAP_ENABLED";
  public static final String ENV_NATIVE_MIN_BATCH_SIZE = "PINOT_NATIVE_MIN_BATCH_SIZE";

  // Default values
  private static final boolean DEFAULT_NATIVE_ENABLED = true;
  private static final boolean DEFAULT_AGGREGATION_ENABLED = true;
  private static final boolean DEFAULT_DICTIONARY_ENABLED = true;
  private static final boolean DEFAULT_BITMAP_ENABLED = true;
  private static final int DEFAULT_MIN_BATCH_SIZE = 1000;

  // Cached configuration values
  private static volatile boolean _nativeEnabled;
  private static volatile boolean _aggregationEnabled;
  private static volatile boolean _dictionaryEnabled;
  private static volatile boolean _bitmapEnabled;
  private static volatile int _minBatchSize;
  private static volatile boolean _initialized = false;

  private NativeConfig() {
  }

  /**
   * Initializes configuration from system properties and environment variables.
   * Called automatically on first access.
   */
  private static synchronized void initialize() {
    if (_initialized) {
      return;
    }

    _nativeEnabled = getBooleanConfig(NATIVE_ENABLED, ENV_NATIVE_ENABLED, DEFAULT_NATIVE_ENABLED);
    _aggregationEnabled = getBooleanConfig(NATIVE_AGGREGATION_ENABLED, ENV_NATIVE_AGGREGATION_ENABLED,
        DEFAULT_AGGREGATION_ENABLED);
    _dictionaryEnabled = getBooleanConfig(NATIVE_DICTIONARY_ENABLED, ENV_NATIVE_DICTIONARY_ENABLED,
        DEFAULT_DICTIONARY_ENABLED);
    _bitmapEnabled = getBooleanConfig(NATIVE_BITMAP_ENABLED, ENV_NATIVE_BITMAP_ENABLED, DEFAULT_BITMAP_ENABLED);
    _minBatchSize = getIntConfig(NATIVE_MIN_BATCH_SIZE, ENV_NATIVE_MIN_BATCH_SIZE, DEFAULT_MIN_BATCH_SIZE);

    _initialized = true;

    // Log configuration
    if (_nativeEnabled && NativeLibraryLoader.isAvailable()) {
      LOGGER.info("Native optimizations enabled - aggregation: {}, dictionary: {}, bitmap: {}, minBatchSize: {}",
          _aggregationEnabled, _dictionaryEnabled, _bitmapEnabled, _minBatchSize);
    } else if (_nativeEnabled) {
      LOGGER.warn("Native optimizations requested but native library not available. "
          + "Falling back to Java implementations.");
    } else {
      LOGGER.info("Native optimizations disabled by configuration");
    }
  }

  private static boolean getBooleanConfig(String sysProp, String envVar, boolean defaultValue) {
    String value = System.getProperty(sysProp);
    if (value == null) {
      value = System.getenv(envVar);
    }
    if (value != null) {
      return Boolean.parseBoolean(value);
    }
    return defaultValue;
  }

  private static int getIntConfig(String sysProp, String envVar, int defaultValue) {
    String value = System.getProperty(sysProp);
    if (value == null) {
      value = System.getenv(envVar);
    }
    if (value != null) {
      try {
        return Integer.parseInt(value);
      } catch (NumberFormatException e) {
        LOGGER.warn("Invalid integer value for {}: {}. Using default: {}", sysProp, value, defaultValue);
      }
    }
    return defaultValue;
  }

  /**
   * Returns true if native optimizations are enabled globally and the library is available.
   */
  public static boolean isNativeEnabled() {
    if (!_initialized) {
      initialize();
    }
    return _nativeEnabled && NativeLibraryLoader.isAvailable();
  }

  /**
   * Returns true if native aggregation functions should be used.
   */
  public static boolean isAggregationEnabled() {
    if (!_initialized) {
      initialize();
    }
    return isNativeEnabled() && _aggregationEnabled;
  }

  /**
   * Returns true if native dictionary operations should be used.
   */
  public static boolean isDictionaryEnabled() {
    if (!_initialized) {
      initialize();
    }
    return isNativeEnabled() && _dictionaryEnabled;
  }

  /**
   * Returns true if native bitmap operations should be used.
   */
  public static boolean isBitmapEnabled() {
    if (!_initialized) {
      initialize();
    }
    return isNativeEnabled() && _bitmapEnabled;
  }

  /**
   * Returns the minimum batch size for using native operations.
   * For arrays smaller than this, Java operations are used to avoid JNI overhead.
   */
  public static int getMinBatchSize() {
    if (!_initialized) {
      initialize();
    }
    return _minBatchSize;
  }

  /**
   * Returns true if native should be used for the given batch size.
   */
  public static boolean shouldUseNative(int batchSize) {
    return isNativeEnabled() && batchSize >= getMinBatchSize();
  }

  /**
   * Returns true if native aggregation should be used for the given batch size.
   */
  public static boolean shouldUseNativeAggregation(int batchSize) {
    return isAggregationEnabled() && batchSize >= getMinBatchSize();
  }

  /**
   * Force re-initialization of configuration. Mainly for testing.
   */
  public static synchronized void reset() {
    _initialized = false;
  }
}

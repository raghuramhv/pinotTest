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
package org.apache.pinot.query.runtime.native;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.concurrent.atomic.AtomicBoolean;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;


/**
 * Native query server implementation using Rust for high-performance
 * scatter-gather with memory backpressure management.
 *
 * Features:
 * - Bounded async channels for inter-process communication
 * - Lock-free backpressure with configurable thresholds
 * - jemalloc-based memory tracking
 * - Efficient gRPC channel pooling
 */
public class NativeQueryServer {
  private static final Logger LOGGER = LoggerFactory.getLogger(NativeQueryServer.class);
  private static final AtomicBoolean _loaded = new AtomicBoolean(false);
  private static final AtomicBoolean _initialized = new AtomicBoolean(false);
  private static volatile boolean _available = false;

  // Memory pressure levels
  public static final int PRESSURE_NORMAL = 0;
  public static final int PRESSURE_MODERATE = 1;
  public static final int PRESSURE_HIGH = 2;
  public static final int PRESSURE_CRITICAL = 3;

  static {
    loadNativeLibrary();
  }

  private static void loadNativeLibrary() {
    if (_loaded.compareAndSet(false, true)) {
      try {
        // Try system library path first
        try {
          System.loadLibrary("pinot_query_server");
          _available = true;
          LOGGER.info("Loaded pinot_query_server from system library path");
          return;
        } catch (UnsatisfiedLinkError e) {
          LOGGER.debug("Could not load from system path: {}", e.getMessage());
        }

        // Try to extract from JAR
        String platform = detectPlatform();
        String libName = getLibraryName();
        String resourcePath = "/native/" + platform + "/" + libName;

        try (InputStream in = NativeQueryServer.class.getResourceAsStream(resourcePath)) {
          if (in != null) {
            File tempFile = File.createTempFile("pinot_query_server", getLibrarySuffix());
            tempFile.deleteOnExit();

            try (OutputStream out = new FileOutputStream(tempFile)) {
              byte[] buffer = new byte[8192];
              int bytesRead;
              while ((bytesRead = in.read(buffer)) != -1) {
                out.write(buffer, 0, bytesRead);
              }
            }

            System.load(tempFile.getAbsolutePath());
            _available = true;
            LOGGER.info("Loaded pinot_query_server from JAR resource: {}", resourcePath);
            return;
          }
        }

        LOGGER.warn("Native query server library not available - using Java fallback");
      } catch (Exception e) {
        LOGGER.warn("Failed to load native query server library: {}", e.getMessage());
      }
    }
  }

  private static String detectPlatform() {
    String os = System.getProperty("os.name").toLowerCase();
    String arch = System.getProperty("os.arch").toLowerCase();

    String osName;
    if (os.contains("linux")) {
      osName = "linux";
    } else if (os.contains("mac") || os.contains("darwin")) {
      osName = "darwin";
    } else if (os.contains("win")) {
      osName = "windows";
    } else {
      osName = "unknown";
    }

    String archName = arch.contains("aarch64") || arch.contains("arm64") ? "aarch64" : "x86_64";
    return osName + "-" + archName;
  }

  private static String getLibraryName() {
    String os = System.getProperty("os.name").toLowerCase();
    if (os.contains("mac") || os.contains("darwin")) {
      return "libpinot_query_server.dylib";
    } else if (os.contains("win")) {
      return "pinot_query_server.dll";
    } else {
      return "libpinot_query_server.so";
    }
  }

  private static String getLibrarySuffix() {
    String os = System.getProperty("os.name").toLowerCase();
    if (os.contains("mac") || os.contains("darwin")) {
      return ".dylib";
    } else if (os.contains("win")) {
      return ".dll";
    } else {
      return ".so";
    }
  }

  /**
   * Check if native query server is available.
   */
  public static boolean isAvailable() {
    return _available;
  }

  /**
   * Initialize the native query server.
   *
   * @param maxMemoryBytes Maximum memory limit (0 for unlimited)
   * @param maxPendingBlocks Maximum pending blocks per mailbox
   * @return true if initialization succeeded
   */
  public static boolean init(long maxMemoryBytes, int maxPendingBlocks) {
    if (!_available) {
      return false;
    }

    if (_initialized.compareAndSet(false, true)) {
      return nativeInit(maxMemoryBytes, maxPendingBlocks);
    }
    return true;
  }

  /**
   * Initialize with default settings.
   */
  public static boolean init() {
    return init(0, 5);
  }

  /**
   * Shutdown the native query server.
   */
  public static void shutdown() {
    if (_available && _initialized.compareAndSet(true, false)) {
      nativeShutdown();
    }
  }

  /**
   * Check if the native query server is initialized.
   */
  public static boolean isInitialized() {
    return _available && nativeIsInitialized();
  }

  // ==================== Memory Backpressure ====================

  /**
   * Get current memory pressure level.
   *
   * @return Pressure level (0=normal, 1=moderate, 2=high, 3=critical)
   */
  public static int getMemoryPressureLevel() {
    return _available ? nativeGetMemoryPressureLevel() : PRESSURE_NORMAL;
  }

  /**
   * Get current memory usage in bytes.
   */
  public static long getMemoryUsed() {
    return _available ? nativeGetMemoryUsed() : 0;
  }

  /**
   * Get reserved memory in bytes.
   */
  public static long getReservedMemory() {
    return _available ? nativeGetReservedMemory() : 0;
  }

  /**
   * Check if queries should be throttled due to memory pressure.
   */
  public static boolean shouldThrottle() {
    return _available && nativeShouldThrottle();
  }

  /**
   * Get number of active queries.
   */
  public static int getActiveQueries() {
    return _available ? nativeGetActiveQueries() : 0;
  }

  // ==================== Mailbox Operations ====================

  /**
   * Create a receiving mailbox.
   *
   * @return Handle to the mailbox (0 on failure)
   */
  public static long createReceivingMailbox(String queryId, int senderStageId, int senderWorkerId,
      int receiverStageId, int receiverWorkerId, int maxPendingBlocks) {
    if (!_available) {
      return 0;
    }
    return nativeCreateReceivingMailbox(queryId, senderStageId, senderWorkerId, receiverStageId, receiverWorkerId,
        maxPendingBlocks);
  }

  /**
   * Close a receiving mailbox.
   */
  public static void closeReceivingMailbox(long handle) {
    if (_available && handle != 0) {
      nativeCloseReceivingMailbox(handle);
    }
  }

  /**
   * Get number of pending blocks in mailbox.
   */
  public static int getPendingBlocks(long handle) {
    return _available ? nativeGetPendingBlocks(handle) : -1;
  }

  /**
   * Check if mailbox is full (at backpressure threshold).
   */
  public static boolean isMailboxFull(long handle) {
    return _available && nativeIsMailboxFull(handle);
  }

  /**
   * Signal early termination on mailbox.
   */
  public static void signalEarlyTermination(long handle) {
    if (_available && handle != 0) {
      nativeSignalEarlyTermination(handle);
    }
  }

  // ==================== Scheduler Operations ====================

  /**
   * Get number of running queries.
   */
  public static int getRunningQueries() {
    return _available ? nativeGetRunningQueries() : 0;
  }

  /**
   * Get number of queued queries.
   */
  public static int getQueuedQueries() {
    return _available ? nativeGetQueuedQueries() : 0;
  }

  /**
   * Cancel a query.
   *
   * @return true if query was cancelled
   */
  public static boolean cancelQuery(String queryId) {
    return _available && nativeCancelQuery(queryId);
  }

  // ==================== Statistics ====================

  /**
   * Get total queries submitted.
   */
  public static long getTotalQueries() {
    return _available ? nativeGetTotalQueries() : 0;
  }

  /**
   * Get total queries completed successfully.
   */
  public static long getCompletedQueries() {
    return _available ? nativeGetCompletedQueries() : 0;
  }

  /**
   * Get total queries failed.
   */
  public static long getFailedQueries() {
    return _available ? nativeGetFailedQueries() : 0;
  }

  /**
   * Get average execution time in milliseconds.
   */
  public static long getAverageExecutionTimeMs() {
    return _available ? nativeGetAverageExecutionTimeMs() : 0;
  }

  /**
   * Get average queue wait time in milliseconds.
   */
  public static long getAverageQueueWaitMs() {
    return _available ? nativeGetAverageQueueWaitMs() : 0;
  }

  /**
   * Get total backpressure events.
   */
  public static long getBackpressureEvents() {
    return _available ? nativeGetBackpressureEvents() : 0;
  }

  /**
   * Get total queries rejected due to memory pressure.
   */
  public static long getRejectedQueries() {
    return _available ? nativeGetRejectedQueries() : 0;
  }

  // ==================== Native Methods ====================

  private static native boolean nativeInit(long maxMemoryBytes, int maxPendingBlocks);
  private static native void nativeShutdown();
  private static native boolean nativeIsInitialized();

  private static native int nativeGetMemoryPressureLevel();
  private static native long nativeGetMemoryUsed();
  private static native long nativeGetReservedMemory();
  private static native boolean nativeShouldThrottle();
  private static native int nativeGetActiveQueries();

  private static native long nativeCreateReceivingMailbox(String queryId, int senderStageId, int senderWorkerId,
      int receiverStageId, int receiverWorkerId, int maxPendingBlocks);
  private static native void nativeCloseReceivingMailbox(long handle);
  private static native int nativeGetPendingBlocks(long handle);
  private static native boolean nativeIsMailboxFull(long handle);
  private static native void nativeSignalEarlyTermination(long handle);

  private static native int nativeGetRunningQueries();
  private static native int nativeGetQueuedQueries();
  private static native boolean nativeCancelQuery(String queryId);

  private static native long nativeGetTotalQueries();
  private static native long nativeGetCompletedQueries();
  private static native long nativeGetFailedQueries();
  private static native long nativeGetAverageExecutionTimeMs();
  private static native long nativeGetAverageQueueWaitMs();
  private static native long nativeGetBackpressureEvents();
  private static native long nativeGetRejectedQueries();
}

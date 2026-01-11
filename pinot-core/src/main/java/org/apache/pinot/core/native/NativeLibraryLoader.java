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

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Locale;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;


/**
 * Handles loading of native Rust libraries for high-performance operations.
 * The native library is extracted from the JAR to a temporary directory and loaded.
 */
public final class NativeLibraryLoader {
  private static final Logger LOGGER = LoggerFactory.getLogger(NativeLibraryLoader.class);

  private static final String LIBRARY_NAME = "pinot_core";
  private static volatile boolean _loaded = false;
  private static volatile boolean _available = false;
  private static volatile Throwable _loadError = null;

  private NativeLibraryLoader() {
  }

  /**
   * Attempts to load the native library.
   * This method is thread-safe and idempotent.
   *
   * @return true if the library was loaded successfully, false otherwise
   */
  public static synchronized boolean load() {
    if (_loaded) {
      return _available;
    }

    _loaded = true;

    try {
      // First, try to load from java.library.path
      try {
        System.loadLibrary(LIBRARY_NAME);
        _available = true;
        LOGGER.info("Loaded native library '{}' from java.library.path", LIBRARY_NAME);
        return true;
      } catch (UnsatisfiedLinkError e) {
        LOGGER.debug("Native library not found in java.library.path, trying to extract from JAR");
      }

      // Try to extract from JAR
      String libraryPath = extractLibraryFromJar();
      if (libraryPath != null) {
        System.load(libraryPath);
        _available = true;
        LOGGER.info("Loaded native library from JAR: {}", libraryPath);
        return true;
      }

      LOGGER.warn("Native library '{}' not available. Native optimizations will be disabled.", LIBRARY_NAME);
      return false;

    } catch (Throwable t) {
      _loadError = t;
      LOGGER.warn("Failed to load native library '{}': {}. Native optimizations will be disabled.",
          LIBRARY_NAME, t.getMessage());
      return false;
    }
  }

  /**
   * Returns true if the native library is available and loaded.
   */
  public static boolean isAvailable() {
    if (!_loaded) {
      load();
    }
    return _available;
  }

  /**
   * Returns the error that occurred during library loading, or null if no error occurred.
   */
  public static Throwable getLoadError() {
    return _loadError;
  }

  /**
   * Extracts the native library from the JAR file to a temporary directory.
   *
   * @return the path to the extracted library, or null if extraction failed
   */
  private static String extractLibraryFromJar() {
    String osName = System.getProperty("os.name").toLowerCase(Locale.ROOT);
    String osArch = System.getProperty("os.arch").toLowerCase(Locale.ROOT);

    String libraryFileName;
    String resourcePath;

    if (osName.contains("linux")) {
      libraryFileName = "lib" + LIBRARY_NAME + ".so";
      resourcePath = "/native/linux-" + normalizeArch(osArch) + "/" + libraryFileName;
    } else if (osName.contains("mac") || osName.contains("darwin")) {
      libraryFileName = "lib" + LIBRARY_NAME + ".dylib";
      resourcePath = "/native/macos-" + normalizeArch(osArch) + "/" + libraryFileName;
    } else if (osName.contains("win")) {
      libraryFileName = LIBRARY_NAME + ".dll";
      resourcePath = "/native/windows-" + normalizeArch(osArch) + "/" + libraryFileName;
    } else {
      LOGGER.warn("Unsupported operating system: {}", osName);
      return null;
    }

    LOGGER.debug("Looking for native library at resource path: {}", resourcePath);

    try (InputStream is = NativeLibraryLoader.class.getResourceAsStream(resourcePath)) {
      if (is == null) {
        LOGGER.debug("Native library not found in JAR at: {}", resourcePath);
        return null;
      }

      // Create temporary directory
      Path tempDir = Files.createTempDirectory("pinot-native");
      tempDir.toFile().deleteOnExit();

      File tempFile = new File(tempDir.toFile(), libraryFileName);
      tempFile.deleteOnExit();

      // Extract library to temp file
      try (FileOutputStream fos = new FileOutputStream(tempFile)) {
        byte[] buffer = new byte[8192];
        int bytesRead;
        while ((bytesRead = is.read(buffer)) != -1) {
          fos.write(buffer, 0, bytesRead);
        }
      }

      // Make executable on Unix systems
      if (!osName.contains("win")) {
        tempFile.setExecutable(true);
      }

      return tempFile.getAbsolutePath();

    } catch (IOException e) {
      LOGGER.warn("Failed to extract native library from JAR: {}", e.getMessage());
      return null;
    }
  }

  /**
   * Normalizes architecture names to standard form.
   */
  private static String normalizeArch(String arch) {
    if (arch.contains("amd64") || arch.contains("x86_64")) {
      return "x86_64";
    } else if (arch.contains("aarch64") || arch.contains("arm64")) {
      return "aarch64";
    } else if (arch.contains("x86") || arch.contains("i386") || arch.contains("i686")) {
      return "x86";
    }
    return arch;
  }
}

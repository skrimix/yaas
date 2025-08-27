import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

class LogInfo {
  final DateTime timestamp;
  final LogLevel level;
  final String target;
  final String message;
  final LogKind kind;
  final Map<String, String>? fields;
  final SpanTrace? spanTrace;

  LogInfo({
    required this.timestamp,
    required this.level,
    required this.target,
    required this.message,
    required this.kind,
    this.fields,
    this.spanTrace,
  });

  Color get levelColor {
    switch (level) {
      case LogLevel.trace:
        return const Color(0xFF9CA3AF); // Gray
      case LogLevel.debug:
        return const Color(0xFF3B82F6); // Blue
      case LogLevel.info:
        return const Color(0xFF10B981); // Green
      case LogLevel.warn:
        return const Color(0xFFF59E0B); // Yellow
      case LogLevel.error:
        return const Color(0xFFEF4444); // Red
    }
  }

  String get levelString {
    switch (level) {
      case LogLevel.trace:
        return 'TRACE';
      case LogLevel.debug:
        return 'DEBUG';
      case LogLevel.info:
        return 'INFO';
      case LogLevel.warn:
        return 'WARN';
      case LogLevel.error:
        return 'ERROR';
    }
  }

  bool matchesSearch(String query) {
    if (query.isEmpty) return true;
    final lowerQuery = query.toLowerCase();

    // Search in message, target, and level
    if (message.toLowerCase().contains(lowerQuery) ||
        target.toLowerCase().contains(lowerQuery) ||
        levelString.toLowerCase().contains(lowerQuery)) {
      return true;
    }

    // Search in fields
    if (fields != null) {
      for (final entry in fields!.entries) {
        if (entry.key.toLowerCase().contains(lowerQuery) ||
            entry.value.toLowerCase().contains(lowerQuery)) {
          return true;
        }
      }
    }

    // Search in span trace parameters
    if (spanTrace != null) {
      for (final span in spanTrace!.spans) {
        if (span.name.toLowerCase().contains(lowerQuery) ||
            span.target.toLowerCase().contains(lowerQuery)) {
          return true;
        }
        if (span.parameters != null) {
          for (final entry in span.parameters!.entries) {
            if (entry.key.toLowerCase().contains(lowerQuery) ||
                entry.value.toLowerCase().contains(lowerQuery)) {
              return true;
            }
          }
        }
      }
    }

    return false;
  }
}

class LogState extends ChangeNotifier {
  final List<LogInfo> _logs = [];
  final int _maxLogs = 5000; // Keep last x logs in memory

  // Filtering state
  Set<LogLevel> _enabledLevels = {
    LogLevel.trace,
    LogLevel.debug,
    LogLevel.info,
    LogLevel.warn,
    LogLevel.error,
  };
  String _searchQuery = '';
  String _targetFilter = '';
  bool _showSpanEvents = false; // Hide span events by default

  // UI state
  bool _autoScroll = true;

  // Getters
  List<LogInfo> get logs => _filteredLogs();
  Set<LogLevel> get enabledLevels => Set.from(_enabledLevels);
  String get searchQuery => _searchQuery;
  String get targetFilter => _targetFilter;
  bool get showSpanEvents => _showSpanEvents;
  bool get autoScroll => _autoScroll;

  // Get total log count respecting the span events filter
  int get logCount => _showSpanEvents
      ? _logs.length
      : _logs.where((log) => !_isSpanEvent(log)).length;

  LogState() {
    // Receive log entries from Rust
    LogBatch.rustSignalStream.listen((event) {
      final logEntries = event.message.entries;

      for (final logEntry in logEntries) {
        final logInfo = LogInfo(
          timestamp:
              DateTime.fromMillisecondsSinceEpoch(logEntry.timestamp.toInt()),
          level: logEntry.level,
          target: logEntry.target,
          message: logEntry.message,
          kind: logEntry.kind,
          fields: logEntry.fields,
          spanTrace: logEntry.spanTrace,
        );

        _logs.add(logInfo);
      }

      _cleanupOldLogs();
      notifyListeners();
    });
  }

  void _cleanupOldLogs() {
    if (_logs.length > _maxLogs) {
      final removeCount = _logs.length - _maxLogs;
      _logs.removeRange(0, removeCount);
    }
  }

  List<LogInfo> _filteredLogs() {
    return _logs.where((log) {
      if (!_showSpanEvents && _isSpanEvent(log)) {
        return false;
      }

      if (!_enabledLevels.contains(log.level)) {
        return false;
      }

      if (_targetFilter.isNotEmpty &&
          !log.target.toLowerCase().contains(_targetFilter.toLowerCase())) {
        return false;
      }

      if (!log.matchesSearch(_searchQuery)) {
        return false;
      }

      return true;
    }).toList();
  }

  bool _isSpanEvent(LogInfo log) {
    return log.kind == LogKind.spanNew || log.kind == LogKind.spanClose;
  }

  // Filter controls
  void toggleLogLevel(LogLevel level) {
    if (_enabledLevels.contains(level)) {
      _enabledLevels.remove(level);
    } else {
      _enabledLevels.add(level);
    }
    notifyListeners();
  }

  void setSearchQuery(String query) {
    _searchQuery = query;
    notifyListeners();
  }

  void setTargetFilter(String filter) {
    _targetFilter = filter;
    notifyListeners();
  }

  void toggleSpanEvents() {
    _showSpanEvents = !_showSpanEvents;
    notifyListeners();
  }

  void clearFilters() {
    _enabledLevels = {
      LogLevel.trace,
      LogLevel.debug,
      LogLevel.info,
      LogLevel.warn,
      LogLevel.error,
    };
    _searchQuery = '';
    _targetFilter = '';
    // Not resetting _showSpanEvents as it's a separate toggle

    notifyListeners();
  }

  void clearSearch() {
    _searchQuery = '';
    notifyListeners();
  }

  // UI controls
  void setAutoScroll(bool enabled) {
    _autoScroll = enabled;
    notifyListeners();
  }

  void clearCurrentLogs() {
    _logs.clear();
    notifyListeners();
    debugPrint('[LogState] Cleared current session logs');
  }
}

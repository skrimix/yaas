import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:provider/provider.dart';
import '../../providers/log_state.dart';
import '../../src/bindings/bindings.dart';

class LogsScreen extends StatefulWidget {
  const LogsScreen({super.key});

  @override
  State<LogsScreen> createState() => _LogsScreenState();
}

class _LogsScreenState extends State<LogsScreen> {
  final ScrollController _scrollController = ScrollController();
  final TextEditingController _searchController = TextEditingController();
  bool _isAtBottom = true;
  LogState? _logState;

  @override
  void initState() {
    super.initState();

    // Detect if user scrolled up (disables auto-scroll until scrolled to bottom)
    _scrollController.addListener(() {
      final isAtBottom = _scrollController.offset >=
          _scrollController.position.maxScrollExtent - 100;
      if (_isAtBottom != isAtBottom) {
        setState(() => _isAtBottom = isAtBottom);
      }
    });

    // Scroll to bottom when opened
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
      }
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();

    if (_logState == null) {
      _logState = context.read<LogState>();
      _logState!.addListener(_onLogStateChanged);
    }
  }

  @override
  void dispose() {
    _scrollController.dispose();
    _searchController.dispose();
    _logState?.removeListener(_onLogStateChanged);
    super.dispose();
  }

  void _onLogStateChanged() {
    final logState = _logState;
    if (logState != null && logState.autoScroll && _isAtBottom) {
      // Only auto-scroll when user is at bottom
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_scrollController.hasClients) {
          // Note: animating here breaks _isAtBottom detection on high-rate events
          _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
        }
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Column(
        children: [
          // Header with controls
          _buildHeader(),
          // Log viewer
          Expanded(child: _buildLogViewer()),
          // Footer with status
          _buildFooter(),
        ],
      ),
    );
  }

  Widget _buildHeader() {
    return Container(
      padding: const EdgeInsets.all(12.0),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainer,
        border: Border(
          bottom: BorderSide(
            color: Theme.of(context).colorScheme.outline.withValues(alpha: 0.2),
          ),
        ),
      ),
      child: Column(
        children: [
          // Top row: search and controls
          Row(
            children: [
              Expanded(child: _buildSearchField()),
              const SizedBox(width: 8),
              _buildControlButtons(),
            ],
          ),
          const SizedBox(height: 8),
          // Bottom row: filter chips
          _buildFilterChips(),
        ],
      ),
    );
  }

  Widget _buildSearchField() {
    return Consumer<LogState>(
      builder: (context, logState, child) {
        return Tooltip(
          message:
              'Search logs by level, message, target, or span id. Examples: "error", "info", "adb", "connect", "13"',
          child: TextField(
            controller: _searchController,
            decoration: InputDecoration(
              hintText: 'Search logs...',
              prefixIcon: const Icon(Icons.search),
              suffixIcon: _searchController.text.isNotEmpty
                  ? IconButton(
                      icon: const Icon(Icons.clear),
                      onPressed: () {
                        _searchController.clear();
                        logState.setSearchQuery('');
                      },
                    )
                  : null,
              border: const OutlineInputBorder(),
              contentPadding:
                  const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            ),
            onChanged: (value) => logState.setSearchQuery(value),
          ),
        );
      },
    );
  }

  Widget _buildControlButtons() {
    return Consumer<LogState>(
      builder: (context, logState, child) {
        return Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            // Auto-scroll toggle
            // IconButton(
            //   icon: Icon(logState.autoScroll
            //       ? Icons.vertical_align_bottom
            //       : Icons.vertical_align_center),
            //   tooltip: logState.autoScroll
            //       ? 'Disable auto-scroll'
            //       : 'Enable auto-scroll',
            //   onPressed: () => logState.setAutoScroll(!logState.autoScroll),
            //   color: logState.autoScroll
            //       ? Theme.of(context).colorScheme.primary
            //       : null,
            // ),
            // Clear logs
            IconButton(
              icon: const Icon(Icons.clear_all),
              tooltip: 'Clear current logs',
              onPressed: () => _showClearLogsDialog(context),
            ),
            // More options
            PopupMenuButton(
              itemBuilder: (context) => [
                // Export logs
                PopupMenuItem(
                  child: const Row(
                    children: [
                      Icon(Icons.file_copy, size: 16),
                      SizedBox(width: 8),
                      Text('Export logs'),
                    ],
                  ),
                  onTap: () => _exportLogs(logState),
                ),
                // Open logs directory
                PopupMenuItem(
                  child: const Row(
                    children: [
                      Icon(Icons.folder_open, size: 16),
                      SizedBox(width: 8),
                      Text('Open logs directory'),
                    ],
                  ),
                  onTap: () => _openLogsDirectory(),
                ),
                // Clear filters
                PopupMenuItem(
                  child: const Row(
                    children: [
                      Icon(Icons.filter_alt_off, size: 16),
                      SizedBox(width: 8),
                      Text('Clear filters'),
                    ],
                  ),
                  onTap: () {
                    logState.clearFilters();
                    _searchController.clear();
                  },
                ),
              ],
            ),
          ],
        );
      },
    );
  }

  Widget _buildFilterChips() {
    return Consumer<LogState>(
      builder: (context, logState, child) {
        return Wrap(
          spacing: 8,
          runSpacing: 4,
          children: [
            // Log level filters
            for (final level in LogLevel.values)
              FilterChip(
                label: Text(
                  level.name.toUpperCase(),
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: logState.enabledLevels.contains(level)
                        ? FontWeight.w600
                        : FontWeight.normal,
                  ),
                ),
                selected: logState.enabledLevels.contains(level),
                onSelected: (_) => logState.toggleLogLevel(level),
                backgroundColor: logState.enabledLevels.contains(level)
                    ? Theme.of(context)
                        .colorScheme
                        .primary
                        .withValues(alpha: 0.1)
                    : Theme.of(context)
                        .colorScheme
                        .outline
                        .withValues(alpha: 0.1),
                selectedColor: Theme.of(context)
                    .colorScheme
                    .primary
                    .withValues(alpha: 0.2),
                checkmarkColor: Theme.of(context).colorScheme.primary,
                side: logState.enabledLevels.contains(level)
                    ? BorderSide(
                        color: Theme.of(context)
                            .colorScheme
                            .primary
                            .withValues(alpha: 0.3))
                    : BorderSide(
                        color: Theme.of(context)
                            .colorScheme
                            .outline
                            .withValues(alpha: 0.3)),
                materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              ),
            // Separator
            Container(
              width: 1,
              height: 24,
              color:
                  Theme.of(context).colorScheme.outline.withValues(alpha: 0.3),
              margin: const EdgeInsets.symmetric(horizontal: 4),
            ),
            // Span events toggle
            Tooltip(
              message:
                  'Show/hide span creation and destruction events. Spans track execution flow.',
              child: FilterChip(
                label: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(
                      Icons.timeline,
                      size: 14,
                      color: logState.showSpanEvents
                          ? Theme.of(context).colorScheme.primary
                          : Theme.of(context).colorScheme.outline,
                    ),
                    const SizedBox(width: 4),
                    // TODO: add a setting for not recording span events
                    Text(
                      'SPANS',
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: logState.showSpanEvents
                            ? FontWeight.w600
                            : FontWeight.normal,
                      ),
                    ),
                  ],
                ),
                selected: logState.showSpanEvents,
                onSelected: (_) => logState.toggleSpanEvents(),
                backgroundColor: logState.showSpanEvents
                    ? Theme.of(context)
                        .colorScheme
                        .secondary
                        .withValues(alpha: 0.1)
                    : Theme.of(context)
                        .colorScheme
                        .outline
                        .withValues(alpha: 0.1),
                selectedColor: Theme.of(context)
                    .colorScheme
                    .secondary
                    .withValues(alpha: 0.2),
                checkmarkColor: Theme.of(context).colorScheme.secondary,
                side: logState.showSpanEvents
                    ? BorderSide(
                        color: Theme.of(context)
                            .colorScheme
                            .secondary
                            .withValues(alpha: 0.3))
                    : BorderSide(
                        color: Theme.of(context)
                            .colorScheme
                            .outline
                            .withValues(alpha: 0.3)),
                materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              ),
            ),
          ],
        );
      },
    );
  }

  Widget _buildLogViewer() {
    return Consumer<LogState>(
      builder: (context, logState, child) {
        final logs = logState.logs;

        if (logs.isEmpty) {
          return Center(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Icon(
                  Icons.terminal,
                  size: 64,
                  color: Theme.of(context).colorScheme.outline,
                ),
                const SizedBox(height: 16),
                Text(
                  'No logs to display',
                  style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        color: Theme.of(context).colorScheme.outline,
                      ),
                ),
                const SizedBox(height: 8),
                Text(
                  'Log messages will appear here as they are generated',
                  style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                        color: Theme.of(context).colorScheme.outline,
                      ),
                ),
              ],
            ),
          );
        }

        return ListView.builder(
          controller: _scrollController,
          padding: const EdgeInsets.all(8.0),
          itemCount: logs.length,
          itemBuilder: (context, index) {
            final log = logs[index];
            return _buildLogEntry(log, index);
          },
        );
      },
    );
  }

  Widget _buildLogEntry(LogInfo log, int index) {
    final isEvenRow = index.isEven;
    final displayMessage = _getDisplayMessage(log);
    final isSpecialMessage = displayMessage == '<no message>' ||
        (displayMessage.startsWith('<') && displayMessage.endsWith('>'));

    return Material(
      color: isEvenRow
          ? Theme.of(context).colorScheme.surface
          : Theme.of(context)
              .colorScheme
              .surfaceContainer
              .withValues(alpha: 0.3),
      child: InkWell(
        onTap: () => _showLogDetails(log),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          decoration: BoxDecoration(
            border: Border(
              left: BorderSide(
                color: log.levelColor,
                width: 3,
              ),
            ),
          ),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Timestamp
              SizedBox(
                width: 100,
                child: Text(
                  _formatTimestamp(log.timestamp),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: Theme.of(context).colorScheme.outline,
                        fontFamily: 'monospace',
                      ),
                ),
              ),
              const SizedBox(width: 8),
              // Level badge
              SizedBox(
                width: 60,
                child: Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: log.levelColor.withValues(alpha: 0.2),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Center(
                    child: Text(
                      log.levelString,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                            color: log.levelColor,
                            fontWeight: FontWeight.bold,
                            fontSize: 10,
                          ),
                    ),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              // Target
              SizedBox(
                width: 140,
                child: Tooltip(
                  message: log.target,
                  child: Text(
                    log.target,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Theme.of(context).colorScheme.primary,
                          fontFamily: 'monospace',
                        ),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              // Message
              Expanded(
                child: Tooltip(
                  message: displayMessage,
                  child: Text(
                    displayMessage,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                          fontFamily: 'monospace',
                          fontStyle: isSpecialMessage ? FontStyle.italic : null,
                          color: isSpecialMessage
                              ? Theme.of(context).colorScheme.outline
                              : null,
                        ),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ),
              // Show indicator if there are additional fields except "location"
              if ((log.fields?.keys.where((k) => k != 'location').length ?? 0) >
                  0) ...[
                const SizedBox(width: 8),
                Icon(
                  Icons.more_horiz,
                  size: 16,
                  color: Theme.of(context).colorScheme.outline,
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildFooter() {
    return Consumer<LogState>(
      builder: (context, logState, child) {
        return Container(
          height: 48,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.surfaceContainer,
            border: Border(
              top: BorderSide(
                color: Theme.of(context)
                    .colorScheme
                    .outline
                    .withValues(alpha: 0.2),
              ),
            ),
          ),
          child: Row(
            children: [
              Text(
                '${logState.logs.length} logs displayed (${logState.logCount} total)',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const Spacer(),
              if (!_isAtBottom && logState.autoScroll)
                TextButton.icon(
                  icon: const Icon(Icons.keyboard_arrow_down, size: 16),
                  label: const Text('Scroll to bottom',
                      style: TextStyle(fontSize: 12)),
                  onPressed: () {
                    _scrollController.animateTo(
                      _scrollController.position.maxScrollExtent,
                      duration: const Duration(milliseconds: 300),
                      curve: Curves.easeOut,
                    );
                  },
                ),
            ],
          ),
        );
      },
    );
  }

  void _showLogDetails(LogInfo log) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Log Entry - ${log.levelString}'),
        content: SizedBox(
          width: 800,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                _buildDetailRow(
                    'Timestamp', _formatTimestampDetailed(log.timestamp)),
                _buildDetailRow('Level', log.levelString),
                _buildDetailRow('Target', log.target),
                _buildDetailRow('Message', log.message),

                // Show location information as a normal property
                if (log.fields?.containsKey('location') == true)
                  _buildDetailRow('Location', log.fields!['location']!),

                // Show span trace with visual hierarchy
                if (log.spanTrace?.spans.isNotEmpty == true) ...[
                  const SizedBox(height: 8),
                  _buildSpanTraceSection(log.spanTrace!, log),
                ],
                // Only show fields header if there are regular fields (excluding special ones)
                ...() {
                  final regularFields = log.fields?.entries
                          .where(
                              (e) => e.key != 'location' && e.key != 'return')
                          .toList() ??
                      [];

                  if (regularFields.isNotEmpty) {
                    return [
                      const SizedBox(height: 4),
                      Container(
                        padding: const EdgeInsets.symmetric(vertical: 4),
                        decoration: BoxDecoration(
                          border: Border(
                            bottom: BorderSide(
                              color: Theme.of(context)
                                  .colorScheme
                                  .outline
                                  .withValues(alpha: 0.3),
                              width: 1,
                            ),
                          ),
                        ),
                        child: Text(
                          'Fields',
                          style: TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 16,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                        ),
                      ),
                      const SizedBox(height: 4),
                      ...regularFields
                          .map((e) => _buildDetailRow(e.key, e.value)),
                    ];
                  } else {
                    return <Widget>[];
                  }
                }(),
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () {
              final formattedTimestamp =
                  _formatTimestampDetailed(log.timestamp);
              final buffer = StringBuffer();
              buffer.writeln('Timestamp: $formattedTimestamp');
              buffer.writeln('Level: ${log.levelString}');
              buffer.writeln('Target: ${log.target}');
              buffer.writeln('Message: ${log.message}');

              // Include location information
              if (log.fields?.containsKey('location') == true) {
                buffer.writeln('Location: ${log.fields!['location']!}');
              }

              // Include span trace with proper formatting
              if (log.spanTrace?.spans.isNotEmpty == true) {
                buffer.writeln('Span Trace:');
                for (int i = 0; i < log.spanTrace!.spans.length; i++) {
                  final span = log.spanTrace!.spans[i];
                  final isLast = i == log.spanTrace!.spans.length - 1;
                  final spanName = '${span.target}::${span.name}';

                  if (span.parameters?.isNotEmpty == true) {
                    final params = span.parameters!.entries
                        .map((e) => '${e.key}=${e.value}')
                        .join(' ');
                    buffer.writeln(
                        '  ${isLast ? '└─' : '├─'} $spanName [id: ${span.id}] with $params');
                  } else {
                    buffer.writeln(
                        '  ${isLast ? '└─' : '├─'} $spanName [id: ${span.id}]');
                  }
                }
              }

              if (log.fields?.isNotEmpty == true) {
                final regularFields = log.fields!.entries
                    .where((e) => e.key != 'location' && e.key != 'return')
                    .toList();

                if (regularFields.isNotEmpty) {
                  buffer.writeln('\nFields:');
                  for (final entry in regularFields) {
                    buffer.writeln('  ${entry.key}: ${entry.value}');
                  }
                }
              }

              Clipboard.setData(ClipboardData(text: buffer.toString()));
              Navigator.of(context).pop();

              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(content: Text('Log entry copied to clipboard')),
              );
            },
            child: const Text('Copy'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  Widget _buildDetailRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 80,
            child: Text(
              '$label:',
              style: const TextStyle(fontWeight: FontWeight.w500),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: const TextStyle(fontFamily: 'monospace'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildSpanTraceSection(SpanTrace spanTrace, LogInfo log) {
    final spans = spanTrace.spans;

    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Theme.of(context)
            .colorScheme
            .surfaceContainer
            .withValues(alpha: 0.3),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(
          color: Theme.of(context).colorScheme.outline.withValues(alpha: 0.15),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.account_tree_outlined,
                size: 16,
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
              const SizedBox(width: 8),
              Text(
                'Span Trace:',
                style: TextStyle(
                  fontWeight: FontWeight.w600,
                  fontSize: 13,
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          ...spans.asMap().entries.map((entry) {
            final index = entry.key;
            final span = entry.value;
            final isLast = index == spans.length - 1;

            return IntrinsicHeight(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  // Visual connector
                  SizedBox(
                    width: 20,
                    child: Column(
                      children: [
                        Container(
                          width: 6,
                          height: 6,
                          margin: const EdgeInsets.only(top: 8),
                          decoration: BoxDecoration(
                            color: isLast
                                ? Theme.of(context).colorScheme.primary
                                : Theme.of(context)
                                    .colorScheme
                                    .outline
                                    .withValues(alpha: 0.6),
                            shape: BoxShape.circle,
                          ),
                        ),
                        if (!isLast)
                          Expanded(
                            child: Container(
                              width: 1,
                              margin: const EdgeInsets.only(top: 4, bottom: 4),
                              color: Theme.of(context)
                                  .colorScheme
                                  .outline
                                  .withValues(alpha: 0.3),
                            ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  // Span content
                  Expanded(
                    child: Container(
                      padding: const EdgeInsets.symmetric(
                          vertical: 6, horizontal: 10),
                      margin: const EdgeInsets.only(bottom: 8),
                      decoration: BoxDecoration(
                        color: isLast
                            ? Theme.of(context)
                                .colorScheme
                                .primaryContainer
                                .withValues(alpha: 0.3)
                            : Theme.of(context).colorScheme.surface,
                        borderRadius: BorderRadius.circular(6),
                        border: isLast
                            ? Border.all(
                                color: Theme.of(context)
                                    .colorScheme
                                    .primary
                                    .withValues(alpha: 0.2),
                              )
                            : Border.all(
                                color: Theme.of(context)
                                    .colorScheme
                                    .outline
                                    .withValues(alpha: 0.15),
                              ),
                      ),
                      child: _buildStructuredSpanContent(span, isLast, log),
                    ),
                  ),
                ],
              ),
            );
          }),
        ],
      ),
    );
  }

  Widget _buildStructuredSpanContent(SpanInfo span, bool isLast, LogInfo log) {
    final functionName = '${span.target}::${span.name}';
    final hasReturnValue = isLast && log.fields?.containsKey('return') == true;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Top row: function name (and "returned" marker) with span ID + filter button on the right
        if (hasReturnValue) ...[
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: SelectableText(
                  '$functionName returned:',
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12,
                    color: Theme.of(context).colorScheme.onSurface,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              _buildSpanIdActions(span.id),
            ],
          ),
          const SizedBox(height: 4),
          Padding(
            padding: const EdgeInsets.only(left: 12),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              decoration: BoxDecoration(
                color: Theme.of(context)
                    .colorScheme
                    .tertiaryContainer
                    .withValues(alpha: 0.3),
                borderRadius: BorderRadius.circular(4),
                border: Border.all(
                  color: Theme.of(context)
                      .colorScheme
                      .tertiary
                      .withValues(alpha: 0.3),
                ),
              ),
              child: SelectableText(
                log.fields!['return']!,
                style: TextStyle(
                  fontFamily: 'monospace',
                  fontSize: 11,
                  color: Theme.of(context).colorScheme.onTertiaryContainer,
                  fontWeight: FontWeight.w500,
                ),
              ),
            ),
          ),
        ] else ...[
          // Regular function name with trailing span ID + filter button
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: SelectableText(
                  functionName,
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12,
                    color: Theme.of(context).colorScheme.onSurface,
                    fontWeight: isLast ? FontWeight.w600 : FontWeight.w500,
                  ),
                ),
              ),
              _buildSpanIdActions(span.id),
            ],
          ),
        ],
        if (span.parameters?.isNotEmpty == true) ...[
          const SizedBox(height: 4),
          // Parameters - structured display
          Padding(
            padding: const EdgeInsets.only(left: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: span.parameters!.entries.map((entry) {
                final paramName = entry.key;
                final paramValue = entry.value;

                return Padding(
                  padding: const EdgeInsets.only(bottom: 2),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Container(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 4, vertical: 1),
                        decoration: BoxDecoration(
                          color: Theme.of(context)
                              .colorScheme
                              .secondaryContainer
                              .withValues(alpha: 0.4),
                          borderRadius: BorderRadius.circular(3),
                        ),
                        child: Text(
                          paramName,
                          style: TextStyle(
                            fontFamily: 'monospace',
                            fontSize: 10,
                            color: Theme.of(context)
                                .colorScheme
                                .onSecondaryContainer,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                      ),
                      const SizedBox(width: 6),
                      Expanded(
                        child: SelectableText(
                          paramValue.isEmpty ? '(empty)' : paramValue,
                          style: TextStyle(
                            fontFamily: 'monospace',
                            fontSize: 11,
                            color: paramValue.isEmpty
                                ? Theme.of(context).colorScheme.outline
                                : Theme.of(context)
                                    .colorScheme
                                    .onSurfaceVariant,
                            fontStyle:
                                paramValue.isEmpty ? FontStyle.italic : null,
                          ),
                        ),
                      ),
                    ],
                  ),
                );
              }).toList(),
            ),
          ),
        ],
      ],
    );
  }

  void _showClearLogsDialog(BuildContext context) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Clear Logs'),
        content: const Text(
            'This will clear all current session logs. This action cannot be undone.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () {
              final logState = context.read<LogState>();
              logState.clearCurrentLogs();
              logState.clearFilters();
              logState.clearSearch();
              _searchController.clear();
              Navigator.of(context).pop();
            },
            child: const Text('Clear'),
          ),
        ],
      ),
    );
  }

  void _exportLogs(LogState logState) {
    final logs = logState.logs;
    final buffer = StringBuffer();

    for (final log in logs) {
      final formattedTimestamp = _formatTimestampDetailed(log.timestamp);
      buffer.writeln(
          '$formattedTimestamp ${log.levelString} ${log.target}: ${log.message}');

      if (log.fields?.isNotEmpty == true) {
        log.fields!.forEach((key, value) {
          buffer.writeln('  $key: $value');
        });
      }
      buffer.writeln(); // Add empty line between entries
    }

    Clipboard.setData(ClipboardData(text: buffer.toString()));

    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text('${logs.length} logs copied to clipboard')),
    );
  }

  void _openLogsDirectory() async {
    // Listen for logs directory path from Rust
    GetLogsDirectoryResponse.rustSignalStream.take(1).listen((response) async {
      final logsPath = response.message.path;

      try {
        if (Platform.isLinux) {
          await Process.run('xdg-open', [logsPath]);
        } else if (Platform.isMacOS) {
          await Process.run('open', [logsPath]);
        } else if (Platform.isWindows) {
          await Process.run('explorer', [logsPath]);
        } else {
          // Unsupported platform - copy to clipboard
          if (mounted) {
            Clipboard.setData(ClipboardData(text: logsPath));
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text(
                  'Platform not supported. Logs directory path copied to clipboard: $logsPath',
                ),
              ),
            );
          }
          return;
        }
      } catch (e) {
        if (mounted) {
          Clipboard.setData(ClipboardData(text: logsPath));
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                'Unable to open logs directory (copied to clipboard): $logsPath',
              ),
            ),
          );
        }
      }
    });

    // Request the path
    GetLogsDirectoryRequest().sendSignalToRust();
  }

  String _getDisplayMessage(LogInfo log) {
    // Handle return events specially
    if (log.message.isEmpty && log.fields?.containsKey('return') == true) {
      // Get span name from span trace (last span is the current one)
      final spanName = log.spanTrace?.spans.isNotEmpty == true
          ? '${log.spanTrace!.spans.last.target}::${log.spanTrace!.spans.last.name}'
          : 'unknown';
      return '<$spanName return: ${log.fields!['return']!}>';
    }

    // Handle error events specially
    if (log.message.isEmpty && log.fields?.containsKey('error') == true) {
      final spanName = log.spanTrace?.spans.isNotEmpty == true
          ? '${log.spanTrace!.spans.last.target}::${log.spanTrace!.spans.last.name}'
          : 'unknown';
      return '<$spanName error: ${log.fields!['error']!}>';
    }

    // Handle span events using structured kind
    switch (log.kind) {
      case LogKind.spanNew:
        return '<${log.message.substring(10)} created>';
      case LogKind.spanClose:
        return '<${log.message.substring(12)} closed>';
      case LogKind.event:
        // Default handling for events
        return log.message.isEmpty ? '<no message>' : log.message;
    }
  }

  String _formatTimestamp(DateTime timestamp) {
    return DateFormat('HH:mm:ss.SSS').format(timestamp);
  }

  String _formatTimestampDetailed(DateTime timestamp) {
    return DateFormat('yyyy-MM-dd HH:mm:ss.SSS').format(timestamp);
  }

  // Small span ID label + filter button used in details view
  Widget _buildSpanIdActions(String spanId) {
    final displayId = _shortenSpanId(spanId);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Tooltip(
          message: 'Span ID',
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            decoration: BoxDecoration(
              color: Theme.of(context)
                  .colorScheme
                  .surfaceContainerHigh
                  .withValues(alpha: 0.4),
              borderRadius: BorderRadius.circular(4),
              border: Border.all(
                color: Theme.of(context)
                    .colorScheme
                    .outline
                    .withValues(alpha: 0.2),
              ),
            ),
            child: SelectableText(
              displayId,
              style: TextStyle(
                fontFamily: 'monospace',
                fontSize: 10,
                color: Theme.of(context).colorScheme.outline,
              ),
            ),
          ),
        ),
        const SizedBox(width: 6),
        Tooltip(
          message: 'Filter logs by this span ID',
          child: IconButton(
            icon: const Icon(Icons.filter_alt),
            visualDensity: VisualDensity.compact,
            onPressed: () {
              // Set the search field to the span id and apply filter
              final logState = context.read<LogState>();
              _searchController.text = spanId;
              logState.setSearchQuery(spanId);
              // Close the dialog to show filtered results immediately
              Navigator.of(context).pop();
            },
          ),
        ),
      ],
    );
  }

  // Truncate span ID label to last 8 chars if first 8 are zeroes
  String _shortenSpanId(String id) {
    if (id.length >= 8 && id.substring(0, 8) == '00000000') {
      // Return the last 8 characters
      final start = id.length >= 8 ? id.length - 8 : 0;
      return id.substring(start);
    }
    return id;
  }
}

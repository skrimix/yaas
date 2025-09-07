import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:provider/provider.dart';
import '../../providers/log_state.dart';
import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

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

    // Disable auto-scroll automatically when scrolling up
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
      // Keep the search field in sync with the persisted filter state
      _searchController.text = _logState!.searchQuery;
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
          // Note: there was an attempt to animate here, but it broke _isAtBottom detection on high-rate events
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
          _buildHeader(),
          Expanded(child: _buildLogViewer()),
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
          // Top row
          Row(
            children: [
              Expanded(child: _buildSearchField()),
              const SizedBox(width: 8),
              _buildControlButtons(),
            ],
          ),
          const SizedBox(height: 8),
          // Bottom row
          _buildFilterChips(),
        ],
      ),
    );
  }

  Widget _buildSearchField() {
    return Consumer<LogState>(
      builder: (context, logState, child) {
        final l10n = AppLocalizations.of(context);
        return Tooltip(
          message: l10n.logsSearchTooltip,
          child: TextField(
            controller: _searchController,
            decoration: InputDecoration(
              hintText: l10n.logsSearchHint,
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
        final l10n = AppLocalizations.of(context);
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
              tooltip: l10n.clearCurrentLogs,
              onPressed: () => _showClearLogsDialog(context),
            ),
            // More options
            PopupMenuButton(
              itemBuilder: (context) => [
                // Export logs
                PopupMenuItem(
                  child: Row(
                    children: [
                      const Icon(Icons.file_copy, size: 16),
                      const SizedBox(width: 8),
                      Text(l10n.exportLogs),
                    ],
                  ),
                  onTap: () => _exportLogs(logState),
                ),
                // Open logs directory
                PopupMenuItem(
                  child: Row(
                    children: [
                      const Icon(Icons.folder_open, size: 16),
                      const SizedBox(width: 8),
                      Text(l10n.openLogsDirectory),
                    ],
                  ),
                  onTap: () => _openLogsDirectory(),
                ),
                // Clear filters
                PopupMenuItem(
                  child: Row(
                    children: [
                      const Icon(Icons.filter_alt_off, size: 16),
                      const SizedBox(width: 8),
                      Text(l10n.clearFilters),
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
        final l10n = AppLocalizations.of(context);
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
              message: l10n.logsSpanEventsTooltip,
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
                    // TODO: add a setting for not recording span events?
                    Text(
                      l10n.spansLabel,
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
        final l10n = AppLocalizations.of(context);

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
                  l10n.noLogsToDisplay,
                  style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        color: Theme.of(context).colorScheme.outline,
                      ),
                ),
                const SizedBox(height: 8),
                Text(
                  l10n.logsAppearHere,
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
    final l10n = AppLocalizations.of(context);
    final isSpecialMessage = displayMessage == '<${l10n.noMessage}>' ||
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
              final l10n = AppLocalizations.of(context);
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
                buffer.writeln(l10n.spanTrace);
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
                SnackBar(content: Text(l10n.logEntryCopied)),
              );
            },
            child: Text(AppLocalizations.of(context).commonCopy),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(AppLocalizations.of(context).commonClose),
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
    final l10n = AppLocalizations.of(context);

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
                l10n.spanTrace,
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
    final l10n = AppLocalizations.of(context);

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
                          paramValue.isEmpty ? l10n.emptyValue : paramValue,
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
      builder: (context) {
        final l10n = AppLocalizations.of(context);
        return AlertDialog(
          title: Text(l10n.clearLogsTitle),
          content: Text(l10n.clearLogsMessage),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(l10n.commonCancel),
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
              child: Text(l10n.commonClear),
            ),
          ],
        );
      },
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
      buffer.writeln();
    }

    Clipboard.setData(ClipboardData(text: buffer.toString()));

    final l10n = AppLocalizations.of(context);
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(l10n.logsCopied(logs.length))),
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
            final l10n = AppLocalizations.of(context);
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text(l10n.logsOpenNotSupported(logsPath)),
              ),
            );
          }
          return;
        }
      } catch (e) {
        if (mounted) {
          Clipboard.setData(ClipboardData(text: logsPath));
          final l10n = AppLocalizations.of(context);
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(l10n.logsOpenFailed(logsPath)),
            ),
          );
        }
      }
    });

    // Request the path
    GetLogsDirectoryRequest().sendSignalToRust();
  }

  String _getDisplayMessage(LogInfo log) {
    final l10n = AppLocalizations.of(context);
    // Handle return events specially
    if (log.message.isEmpty && log.fields?.containsKey('return') == true) {
      // Get span name from span trace (last span is the current one)
      final spanName = log.spanTrace?.spans.isNotEmpty == true
          ? '${log.spanTrace!.spans.last.target}::${log.spanTrace!.spans.last.name}'
          : 'unknown';
      return '<$spanName returned: ${log.fields!['return']!}>';
    }

    // Handle error events specially
    if (log.message.isEmpty && log.fields?.containsKey('error') == true) {
      final spanName = log.spanTrace?.spans.isNotEmpty == true
          ? '${log.spanTrace!.spans.last.target}::${log.spanTrace!.spans.last.name}'
          : 'unknown';
      return '<$spanName ${l10n.errorWord}: ${log.fields!['error']!}>';
    }

    // Handle span events using structured kind
    switch (log.kind) {
      case LogKind.spanNew:
        return '<${log.message.substring(10)} ${l10n.createdWord}>';
      case LogKind.spanClose:
        return '<${log.message.substring(12)} ${l10n.closedWord}>';
      case LogKind.event:
        // Default handling for events
        return log.message.isEmpty ? '<${l10n.noMessage}>' : log.message;
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
    final l10n = AppLocalizations.of(context);
    final displayId = _shortenSpanId(spanId);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Tooltip(
          message: l10n.spanId,
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
          message: l10n.filterBySpanId,
          child: IconButton(
            icon: const Icon(Icons.filter_alt),
            visualDensity: VisualDensity.compact,
            onPressed: () {
              // Set the search field to the span id and apply filter
              final logState = context.read<LogState>();
              _searchController.text = spanId;
              logState.setSearchQuery(spanId);

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
      final start = id.length >= 8 ? id.length - 8 : 0;
      return id.substring(start);
    }
    return id;
  }
}

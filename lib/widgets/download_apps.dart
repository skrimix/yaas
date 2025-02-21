import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:intl/intl.dart';
import '../providers/cloud_apps_state.dart';
import '../messages/all.dart';
import '../providers/device_state.dart';
import 'cloud_app_list.dart';

enum SortOption {
  name,
  date,
  size,
}

class DownloadApps extends StatefulWidget {
  const DownloadApps({super.key});

  @override
  State<DownloadApps> createState() => _DownloadAppsState();
}

class _DownloadAppsState extends State<DownloadApps> {
  SortOption _sortOption = SortOption.name;
  bool _sortAscending = true;
  List<CachedAppData>? _sortedApps;
  String? _lastSortKey;
  final _isSearching = true;
  String _searchQuery = '';
  final _searchController = TextEditingController();
  final _scrollController = ScrollController();
  final Set<String> _selectedFullNames = {};
  bool _showCheckboxes = false;
  bool _showOnlySelected = false;
  String? _lastSearchQuery;

  @override
  void dispose() {
    _searchController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  String _formatSize(int bytes) {
    return FileSize.fromBytes(bytes).toString(
      unit: Unit.auto(
        size: bytes,
        baseType: BaseType.metric,
      ),
      decimals: 2,
    );
  }

  String _formatDate(String utcDate) {
    try {
      final date = DateFormat('yyyy-MM-dd HH:mm').parseUtc(utcDate);
      return DateFormat.yMd().add_jm().format(date.toLocal());
    } catch (e) {
      return utcDate;
    }
  }

  List<CachedAppData> _sortApps(List<CloudApp> apps) {
    final newSortKey = '${_sortOption}_${_sortAscending}_${apps.length}';
    if (_sortedApps != null && newSortKey == _lastSortKey) {
      return _sortedApps!;
    }

    final cachedApps = apps
        .map((app) => CachedAppData(
              app: app,
              formattedSize: _formatSize(app.size.toInt()),
              formattedDate: _formatDate(app.lastUpdated),
            ))
        .toList();

    cachedApps.sort((a, b) {
      int comparison;
      switch (_sortOption) {
        case SortOption.name:
          comparison = a.app.fullName
              .toLowerCase()
              .compareTo(b.app.fullName.toLowerCase());
          break;
        case SortOption.date:
          comparison = a.app.lastUpdated.compareTo(b.app.lastUpdated);
          break;
        case SortOption.size:
          comparison = a.app.size.compareTo(b.app.size);
          break;
      }
      return _sortAscending ? comparison : -comparison;
    });

    _lastSortKey = newSortKey;
    _sortedApps = cachedApps;
    return cachedApps;
  }

  void _resetScroll() {
    if (_scrollController.hasClients) {
      _scrollController.jumpTo(0);
    }
  }

  void _resetSearch() {
    setState(() {
      _searchQuery = '';
      // _isSearching = false;
      _searchController.clear();
    });
  }

  List<CachedAppData> _filterAndSortApps(List<CloudApp> apps) {
    final sortedApps = _sortApps(apps);

    var filtered = sortedApps;

    if (_showOnlySelected) {
      filtered = filtered
          .where((app) => _selectedFullNames.contains(app.app.fullName))
          .toList();
    } else if (_searchQuery.isNotEmpty) {
      final searchTerms = _searchQuery.toLowerCase().split(' ');
      filtered = sortedApps.where((app) {
        // Match if all search terms are present in the full name
        final fullNameLower = app.app.fullName.toLowerCase();
        if (searchTerms.every((term) => fullNameLower.contains(term))) {
          return true;
        }

        // Match if package name contains the search query
        return app.app.packageName
            .toLowerCase()
            .contains(_searchQuery.toLowerCase());
      }).toList();
    }

    // Reset scroll position when search query changes
    if (_lastSearchQuery != _searchQuery) {
      _resetScroll();
      _lastSearchQuery = _searchQuery;
    }

    return filtered;
  }

  void _toggleShowOnlySelected() {
    setState(() {
      _showOnlySelected = !_showOnlySelected;
      if (_showOnlySelected) {
        _resetSearch();
      }
    });
  }

  void _toggleSelection(String fullName) {
    setState(() {
      if (_selectedFullNames.contains(fullName)) {
        _selectedFullNames.remove(fullName);
        if (_selectedFullNames.isEmpty) {
          // _showCheckboxes = false;
          _showOnlySelected = false;
        }
      } else {
        _selectedFullNames.add(fullName);
      }
    });
  }

  void _toggleCheckboxVisibility() {
    setState(() {
      _showCheckboxes = !_showCheckboxes;
      if (!_showCheckboxes) {
        _clearSelection();
      }
    });
  }

  void _clearSelection() {
    setState(() {
      _selectedFullNames.clear();
      _showCheckboxes = false;
      _showOnlySelected = false;
    });
  }

  void _install(String appFullName) {
    TaskRequest(
      type: TaskType.TASK_TYPE_DOWNLOAD_INSTALL,
      params: TaskParams(cloudAppFullName: appFullName),
    ).sendSignalToRust();
  }

  void _download(String appFullName) {
    TaskRequest(
      type: TaskType.TASK_TYPE_DOWNLOAD,
      params: TaskParams(cloudAppFullName: appFullName),
    ).sendSignalToRust();
  }

  Widget _buildSortButton() {
    return PopupMenuButton<(SortOption, bool)>(
      tooltip: 'Sort',
      icon: const Icon(Icons.sort),
      initialValue: (_sortOption, _sortAscending),
      itemBuilder: (context) => [
        const PopupMenuItem(
          enabled: false,
          child: Text('Sort by'),
        ),
        PopupMenuItem(
          value: (SortOption.name, true),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.name && _sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              const Text('Name (A to Z)'),
            ],
          ),
        ),
        PopupMenuItem(
          value: (SortOption.name, false),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.name && !_sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              const Text('Name (Z to A)'),
            ],
          ),
        ),
        PopupMenuItem(
          value: (SortOption.date, true),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.date && _sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              const Text('Date (Oldest first)'),
            ],
          ),
        ),
        PopupMenuItem(
          value: (SortOption.date, false),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.date && !_sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              const Text('Date (Newest first)'),
            ],
          ),
        ),
        PopupMenuItem(
          value: (SortOption.size, true),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.size && _sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              const Text('Size (Smallest first)'),
            ],
          ),
        ),
        PopupMenuItem(
          value: (SortOption.size, false),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.size && !_sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              const Text('Size (Largest first)'),
            ],
          ),
        ),
      ],
      onSelected: (value) {
        setState(() {
          _sortOption = value.$1;
          _sortAscending = value.$2;
          _resetScroll();
        });
      },
    );
  }

  Widget _buildSearchButton() {
    if (_isSearching) {
      return SizedBox(
        width: 350,
        child: TextField(
          controller: _searchController,
          // autofocus: true,
          decoration: InputDecoration(
            hintText: 'Search apps...',
            isDense: true,
            contentPadding:
                const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
            border: const OutlineInputBorder(),
            suffixIcon: IconButton(
              icon: const Icon(Icons.close),
              onPressed: () {
                _resetSearch();
              },
              tooltip: 'Clear search',
            ),
          ),
          onChanged: (value) {
            setState(() {
              _searchQuery = value;
            });
          },
        ),
      );
    }

    return IconButton(
      icon: const Icon(Icons.search),
      tooltip: 'Search',
      onPressed: () {
        setState(() {
          // _isSearching = true;
        });
      },
    );
  }

  @override
  void initState() {
    super.initState();
  }

  Widget _buildFilterButton() {
    final hasSelections = _selectedFullNames.isNotEmpty;

    return IconButton(
      icon: Icon(
        _showOnlySelected ? Icons.filter_list_off : Icons.filter_list,
        color: _showOnlySelected ? Theme.of(context).colorScheme.primary : null,
      ),
      tooltip: _showOnlySelected
          ? 'Show all items'
          : hasSelections
              ? 'Show only selected items'
              : 'Filter (no items selected)',
      onPressed: hasSelections ? _toggleShowOnlySelected : null,
    );
  }

  Widget _buildSelectionSummary(List<CachedAppData> apps) {
    if (_selectedFullNames.isEmpty) return const SizedBox.shrink();

    final selectedApps = apps
        .where((app) => _selectedFullNames.contains(app.app.fullName))
        .toList();
    final totalSize =
        selectedApps.fold<int>(0, (sum, app) => sum + app.app.size.toInt());
    final formattedTotalSize = _formatSize(totalSize);

    return Container(
      padding: const EdgeInsets.all(16.0),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainer,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(12)),
      ),
      child: Row(
        children: [
          Text(
            '${selectedApps.length} selected • $formattedTotalSize total', // TODO:  warn if total size is too large
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const Spacer(),
          FilledButton.icon(
            onPressed: () {
              for (final app in selectedApps) {
                _download(app.app.fullName);
              }
              _clearSelection();
            },
            icon: const Icon(Icons.download),
            label: const Text('Download Selected'),
          ),
          const SizedBox(width: 8),
          Consumer<DeviceState>(
            builder: (context, deviceState, _) {
              return FilledButton.icon(
                onPressed: deviceState.isConnected
                    ? () {
                        for (final app in selectedApps) {
                          _install(app.app.fullName);
                        }
                        _clearSelection();
                      }
                    : null,
                icon: const Icon(Icons.install_mobile),
                label: const Text('Install Selected'),
              );
            },
          ),
          const SizedBox(width: 8),
          IconButton(
            onPressed: _clearSelection,
            icon: const Icon(Icons.close),
            tooltip: 'Clear selection',
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<CloudAppsState>(
      builder: (context, cloudAppsState, _) {
        if (cloudAppsState.isLoading) {
          return const Center(
            child: CircularProgressIndicator(),
          );
        }

        if (cloudAppsState.error != null) {
          return Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  'Error loading apps',
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const SizedBox(height: 8),
                Text(cloudAppsState.error!),
                const SizedBox(height: 16),
                FilledButton.icon(
                  onPressed: () => cloudAppsState.refresh(),
                  icon: const Icon(Icons.refresh),
                  label: const Text('Retry'),
                ),
              ],
            ),
          );
        }

        final filteredAndSortedApps = _filterAndSortApps(cloudAppsState.apps);

        return Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.all(16.0),
                  child: Row(
                    children: [
                      Text(
                        'Available Apps',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      const Spacer(),
                      if (!_showOnlySelected) _buildSearchButton(),
                      if (_showCheckboxes) _buildFilterButton(),
                      IconButton(
                        icon: Icon(_showCheckboxes
                            ? Icons.check_box
                            : Icons.check_box_outline_blank),
                        tooltip: 'Multi-select',
                        onPressed: _toggleCheckboxVisibility,
                      ),
                      _buildSortButton(),
                      IconButton(
                        icon: const Icon(Icons.refresh),
                        tooltip: 'Refresh',
                        onPressed: () => cloudAppsState.refresh(),
                      ),
                    ],
                  ),
                ),
                if (_showOnlySelected && _selectedFullNames.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                    child: Row(
                      children: [
                        Icon(
                          Icons.filter_list,
                          size: 16,
                          color: Theme.of(context).colorScheme.primary,
                        ),
                        const SizedBox(width: 8),
                        Text(
                          'Showing selected items only',
                          style: Theme.of(context)
                              .textTheme
                              .bodyMedium
                              ?.copyWith(
                                color: Theme.of(context).colorScheme.primary,
                              ),
                        ),
                      ],
                    ),
                  ),
                Expanded(
                  child: CloudAppList(
                    apps: filteredAndSortedApps,
                    showCheckboxes: _showCheckboxes,
                    selectedFullNames: _selectedFullNames,
                    scrollController: _scrollController,
                    onSelectionChanged: _toggleSelection,
                    onDownload: _download,
                    onInstall: _install,
                  ),
                ),
                _buildSelectionSummary(filteredAndSortedApps),
              ],
            ),
          ),
        );
      },
    );
  }
}

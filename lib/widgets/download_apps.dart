import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:intl/intl.dart';
import 'package:toastification/toastification.dart';
import '../providers/cloud_apps_state.dart';
import '../messages/all.dart';
import '../providers/device_state.dart';

enum SortOption {
  name,
  date,
  size,
}

class _CachedAppData {
  final CloudApp app;
  final String formattedSize;
  final String formattedDate;

  const _CachedAppData({
    required this.app,
    required this.formattedSize,
    required this.formattedDate,
  });
}

class DownloadApps extends StatefulWidget {
  const DownloadApps({super.key});

  @override
  State<DownloadApps> createState() => _DownloadAppsState();
}

class _DownloadAppsState extends State<DownloadApps> {
  static const _cardPadding =
      EdgeInsets.symmetric(horizontal: 16.0, vertical: 4.0);
  static const _listPadding = EdgeInsets.only(bottom: 24);

  SortOption _sortOption = SortOption.name;
  bool _sortAscending = true;
  List<_CachedAppData>? _sortedApps;
  String? _lastSortKey;
  bool _isSearching = false;
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

  List<_CachedAppData> _sortApps(List<CloudApp> apps) {
    final newSortKey = '${_sortOption}_${_sortAscending}_${apps.length}';
    if (_sortedApps != null && newSortKey == _lastSortKey) {
      return _sortedApps!;
    }

    final cachedApps = apps
        .map((app) => _CachedAppData(
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
      _isSearching = false;
      _searchController.clear();
    });
  }

  List<_CachedAppData> _filterAndSortApps(List<CloudApp> apps) {
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
          autofocus: true,
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
          _isSearching = true;
        });
      },
    );
  }

  @override
  void initState() {
    super.initState();
  }

  Widget _buildAppList(List<CloudApp> apps) {
    final filteredAndSortedApps = _filterAndSortApps(apps);

    if (filteredAndSortedApps.isEmpty) {
      if (_searchQuery.isNotEmpty) {
        return const Center(
          child: Padding(
            padding: EdgeInsets.all(16.0),
            child: Text('No apps match your search'),
          ),
        );
      }
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(16.0),
          child: Text('No apps available'),
        ),
      );
    }

    return ListView.builder(
      controller: _scrollController,
      padding: _listPadding,
      itemCount: filteredAndSortedApps.length,
      prototypeItem: _AppListItem(
        cachedApp: filteredAndSortedApps.first,
        isSelected: _selectedFullNames
            .contains(filteredAndSortedApps.first.app.fullName),
        onSelectionChanged: (selected) =>
            _toggleSelection(filteredAndSortedApps.first.app.fullName),
        showCheckbox: _showCheckboxes,
      ),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      itemBuilder: (context, index) {
        final cachedApp = filteredAndSortedApps[index];
        return _AppListItem(
          cachedApp: cachedApp,
          isSelected: _selectedFullNames.contains(cachedApp.app.fullName),
          onSelectionChanged: (selected) =>
              _toggleSelection(cachedApp.app.fullName),
          showCheckbox: _showCheckboxes,
        );
      },
    );
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

  Widget _buildSelectionSummary(List<_CachedAppData> apps) {
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
            '${selectedApps.length} selected • $formattedTotalSize total',
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const Spacer(),
          FilledButton.icon(
            onPressed: () {
              // TODO: Implement batch download
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
                        // TODO: Implement batch install
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
                      if (_showCheckboxes) _buildFilterButton(),
                      IconButton(
                        icon: Icon(_showCheckboxes
                            ? Icons.check_box
                            : Icons.check_box_outline_blank),
                        tooltip: 'Multi-select',
                        onPressed: _toggleCheckboxVisibility,
                      ),
                      if (!_showOnlySelected) _buildSearchButton(),
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
                  child: _buildAppList(cloudAppsState.apps),
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

class _AppListItem extends StatelessWidget {
  const _AppListItem({
    required this.cachedApp,
    required this.isSelected,
    required this.onSelectionChanged,
    required this.showCheckbox,
  });

  final _CachedAppData cachedApp;
  final bool isSelected;
  final ValueChanged<bool> onSelectionChanged;
  final bool showCheckbox;

  void _copyToClipboard(BuildContext context, String text) {
    Clipboard.setData(ClipboardData(text: text));
    toastification.show(
      type: ToastificationType.success,
      style: ToastificationStyle.flat,
      title: Text('Copied to clipboard'),
      description: Text(text),
      autoCloseDuration: const Duration(seconds: 2),
      backgroundColor: Theme.of(context).colorScheme.surfaceContainer,
      borderSide: BorderSide.none,
      alignment: Alignment.bottomRight,
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
      child: SizedBox(
        child: MenuAnchor(
          menuChildren: [
            MenuItemButton(
              child: const Text('Copy full name'),
              onPressed: () {
                _copyToClipboard(context, cachedApp.app.fullName);
              },
            ),
            MenuItemButton(
              child: const Text('Copy package name'),
              onPressed: () {
                _copyToClipboard(context, cachedApp.app.packageName);
              },
            ),
          ],
          builder: (context, controller, child) {
            return GestureDetector(
              onSecondaryTapUp: (details) {
                controller.open(position: details.localPosition);
              },
              onLongPress: () {
                controller.open();
              },
              onTapUp: (_) {
                if (showCheckbox) {
                  onSelectionChanged(!isSelected);
                }
                controller.close();
              },
              child: ListTile(
                leading: showCheckbox
                    ? Checkbox(
                        value: isSelected,
                        onChanged: (value) =>
                            onSelectionChanged(value ?? false),
                      )
                    : null,
                title: Text(
                  cachedApp.app.fullName,
                  softWrap: false,
                  overflow: TextOverflow.ellipsis,
                ),
                subtitle: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      cachedApp.app.packageName,
                      style: textTheme.bodyMedium?.copyWith(
                        color:
                            textTheme.bodyMedium?.color?.withValues(alpha: 0.6),
                      ),
                    ),
                    Text(
                      'Size: ${cachedApp.formattedSize} • Last Updated: ${cachedApp.formattedDate}',
                      style: textTheme.bodySmall?.copyWith(
                        color:
                            textTheme.bodySmall?.color?.withValues(alpha: 0.6),
                      ),
                    ),
                  ],
                ),
                contentPadding: _DownloadAppsState._cardPadding,
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    IconButton(
                      icon: const Icon(Icons.download),
                      tooltip: 'Download to computer',
                      onPressed: () {
                        // TODO: Implement download functionality
                      },
                    ),
                    const SizedBox(width: 8),
                    Consumer<DeviceState>(
                      builder: (context, deviceState, _) {
                        return IconButton(
                          icon: const Icon(Icons.install_mobile),
                          tooltip: deviceState.isConnected
                              ? 'Install on device'
                              : 'Install on device (not connected)',
                          onPressed: deviceState.isConnected
                              ? () {
                                  // TODO: Implement install functionality
                                }
                              : null,
                        );
                      },
                    ),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    );
  }
}

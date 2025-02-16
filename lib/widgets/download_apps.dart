import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart';
import 'package:intl/intl.dart';
import '../providers/cloud_apps_state.dart';
import '../messages/all.dart';

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
  static const _itemExtent = 88.0;

  SortOption _sortOption = SortOption.name;
  bool _sortAscending = true;
  List<_CachedAppData>? _sortedApps;
  String? _lastSortKey;
  bool _isSearching = false;
  String _searchQuery = '';
  final _searchController = TextEditingController();

  @override
  void dispose() {
    _searchController.dispose();
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

  List<_CachedAppData> _filterAndSortApps(List<CloudApp> apps) {
    final sortedApps = _sortApps(apps);

    if (_searchQuery.isEmpty) {
      return sortedApps;
    }

    final searchTerms = _searchQuery.toLowerCase().split(' ');
    return sortedApps.where((app) {
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
        });
      },
    );
  }

  Widget _buildSearchButton() {
    if (_isSearching) {
      return SizedBox(
        width: 200,
        child: TextField(
          controller: _searchController,
          decoration: InputDecoration(
            hintText: 'Search apps...',
            isDense: true,
            contentPadding:
                const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
            border: const OutlineInputBorder(),
            suffixIcon: IconButton(
              icon: const Icon(Icons.close),
              onPressed: () {
                setState(() {
                  _isSearching = false;
                  _searchQuery = '';
                  _searchController.clear();
                });
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
      padding: _listPadding,
      itemCount: filteredAndSortedApps.length,
      itemExtent: _itemExtent,
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      itemBuilder: (context, index) {
        final cachedApp = filteredAndSortedApps[index];
        return _AppListItem(cachedApp: cachedApp);
      },
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
                      _buildSearchButton(),
                      _buildSortButton(),
                      IconButton(
                        icon: const Icon(Icons.refresh),
                        tooltip: 'Refresh',
                        onPressed: () => cloudAppsState.refresh(),
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: _buildAppList(cloudAppsState.apps),
                ),
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
  });

  final _CachedAppData cachedApp;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final textTheme = theme.textTheme;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
      child: SizedBox(
        height: _DownloadAppsState._itemExtent,
        child: ListTile(
          title: Text(cachedApp.app.fullName),
          subtitle: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                cachedApp.app.packageName,
                style: textTheme.bodyMedium?.copyWith(
                  color: textTheme.bodyMedium?.color?.withValues(alpha: 0.6),
                ),
              ),
              Text(
                'Size: ${cachedApp.formattedSize} • Last Updated: ${cachedApp.formattedDate}',
                style: textTheme.bodySmall?.copyWith(
                  color: textTheme.bodySmall?.color?.withValues(alpha: 0.6),
                ),
              ),
            ],
          ),
          contentPadding: _DownloadAppsState._cardPadding,
          trailing: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              IconButton(
                icon: const Icon(Icons.install_mobile),
                tooltip: 'Install on device',
                onPressed: () {
                  // TODO: Implement install functionality
                },
              ),
              const SizedBox(width: 8),
              IconButton(
                icon: const Icon(Icons.download),
                tooltip: 'Download to computer',
                onPressed: () {
                  // TODO: Implement download functionality
                },
              ),
            ],
          ),
        ),
      ),
    );
  }
}

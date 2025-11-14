import 'dart:async';
import 'dart:math' as math;
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:proper_filesize/proper_filesize.dart' as filesize;
import 'package:intl/intl.dart';
import '../../src/l10n/app_localizations.dart';
import '../../providers/cloud_apps_state.dart';
import '../../providers/app_state.dart';
import '../../src/bindings/bindings.dart';
import '../../providers/device_state.dart';
import '../../providers/settings_state.dart';
import '../app_management/cloud_app_list.dart';

enum SortOption {
  name,
  date,
  size,
}

class DownloadAppsScreen extends StatefulWidget {
  const DownloadAppsScreen({super.key});

  @override
  State<DownloadAppsScreen> createState() => _DownloadAppsScreenState();
}

class _DownloadAppsScreenState extends State<DownloadAppsScreen> {
  SortOption _sortOption = SortOption.name;
  bool _sortAscending = true;
  List<CachedAppData>? _sortedApps;
  String? _lastSortKey;
  final _isSearching = true; // Always true
  String _searchQuery = '';
  final _searchController = TextEditingController();
  final _scrollController = ScrollController();
  final Set<String> _selectedFullNames = {};
  bool _showCheckboxes = false;
  bool _showOnlySelected = false;
  bool _showOnlyFavorites = false;
  String? _lastSearchQuery;
  Timer? _searchDebounceTimer;
  bool _initialized = false;

  @override
  void dispose() {
    _searchDebounceTimer?.cancel();
    _searchController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  String _formatSize(int bytes) {
    return filesize.FileSize.fromBytes(bytes).toString(
      unit: filesize.Unit.auto(
        size: bytes,
        baseType: filesize.BaseType.metric,
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
    final newSortKey =
        '${_sortOption}_${_sortAscending}_${identityHashCode(apps)}_${apps.length}';
    if (_sortedApps != null && newSortKey == _lastSortKey) {
      return _sortedApps!;
    }

    final cachedApps = apps
        .map((app) => CachedAppData(
              app: app,
              formattedSize: _formatSize(app.size.toInt()),
              formattedDate: _formatDate(app.lastUpdated),
              fullNameLower: app.fullName.toLowerCase(),
              packageNameLower: app.packageName.toLowerCase(),
            ))
        .toList();

    cachedApps.sort((a, b) {
      int comparison;
      switch (_sortOption) {
        case SortOption.name:
          comparison = a.fullNameLower.compareTo(b.fullNameLower);
          break;
        case SortOption.date:
          comparison = a.app.lastUpdated.compareTo(b.app.lastUpdated);
          break;
        case SortOption.size:
          comparison = a.app.size.toBigInt().compareTo(b.app.size.toBigInt());
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
    _searchDebounceTimer?.cancel();
    setState(() {
      _searchQuery = '';
      // _isSearching = false;
      _searchController.clear();
    });
    context.read<AppState>().setDownloadSearchQuery('');
  }

  int _calculateSearchScore({
    required String fullNameLower,
    required String packageNameLower,
    required String queryLower,
    required List<String> searchTerms,
    required RegExp? wordQueryRe,
    required List<RegExp>? termWordRes,
  }) {
    int score = 0;

    // Exact full name match (highest priority)
    if (fullNameLower == queryLower) {
      score += 1000;
    }
    // Exact package name match
    else if (packageNameLower == queryLower) {
      score += 900;
    }
    // Full name starts with query
    else if (fullNameLower.startsWith(queryLower)) {
      score += 800;
    }
    // Package name starts with query
    else if (packageNameLower.startsWith(queryLower)) {
      score += 700;
    }
    // Full name contains query as whole word
    else if (wordQueryRe?.hasMatch(fullNameLower) == true) {
      score += 600;
    }
    // Package name contains query as whole word
    else if (wordQueryRe?.hasMatch(packageNameLower) == true) {
      score += 500;
    }
    // All search terms present in full name
    else if (searchTerms.every((term) => fullNameLower.contains(term))) {
      score += 400;
      // Bonus for terms appearing as whole words
      for (final re in termWordRes ?? const <RegExp>[]) {
        if (re.hasMatch(fullNameLower)) score += 50;
      }
    }
    // Full name contains the complete query
    else if (fullNameLower.contains(queryLower)) {
      score += 300;
    }
    // Package name contains the complete query
    else if (packageNameLower.contains(queryLower)) {
      score += 200;
    }

    // Bonus for shorter names (more specific matches)
    if (score > 0) {
      score += math.max(0, 100 - fullNameLower.length);
    }

    return score;
  }

  List<CachedAppData> _filterAndSortApps(List<CloudApp> apps) {
    final sortedApps = _sortApps(apps);

    var filtered = sortedApps;

    if (_showOnlySelected) {
      filtered = filtered
          .where((app) => _selectedFullNames.contains(app.app.fullName))
          .toList();
    } else if (_searchQuery.isNotEmpty) {
      final queryLower = _searchQuery.toLowerCase();
      final searchTerms = queryLower.split(' ');
      // Precompile regexes once per search
      final wordQueryRe = RegExp(r'\b' + RegExp.escape(queryLower) + r'\b');
      final termWordRes = searchTerms
          .where((t) => t.isNotEmpty)
          .map((t) => RegExp(r'\b' + RegExp.escape(t) + r'\b'))
          .toList();

      // Filter and score matching apps
      final matchingApps = <({CachedAppData app, int score})>[];

      for (final app in sortedApps) {
        final fullNameLower = app.fullNameLower;
        final packageNameLower = app.packageNameLower;

        // Check if app matches search criteria
        bool matches = false;
        if (searchTerms.every((term) => fullNameLower.contains(term))) {
          matches = true;
        } else if (packageNameLower.contains(queryLower)) {
          matches = true;
        }

        if (matches) {
          final score = _calculateSearchScore(
            fullNameLower: fullNameLower,
            packageNameLower: packageNameLower,
            queryLower: queryLower,
            searchTerms: searchTerms,
            wordQueryRe: wordQueryRe,
            termWordRes: termWordRes,
          );
          matchingApps.add((app: app, score: score));
        }
      }

      // Sort by search relevance score (highest first)
      matchingApps.sort((a, b) => b.score.compareTo(a.score));
      filtered = matchingApps.map((item) => item.app).toList();
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
    context.read<AppState>().setDownloadShowOnlySelected(_showOnlySelected);
  }

  void _toggleShowOnlyFavorites() {
    setState(() {
      _showOnlyFavorites = !_showOnlyFavorites;
      if (_showOnlyFavorites) {
        _resetSearch();
      }
    });
  }

  void _toggleSelection(String fullName) {
    setState(() {
      if (_selectedFullNames.contains(fullName)) {
        _selectedFullNames.remove(fullName);
        if (_selectedFullNames.isEmpty) {
          _showCheckboxes = false;
          _showOnlySelected = false;
        }
      } else {
        _selectedFullNames.add(fullName);
      }
    });
    final appState = context.read<AppState>();
    appState.setDownloadSelectedFullNames(_selectedFullNames);
    appState.setDownloadShowCheckboxes(_showCheckboxes);
    appState.setDownloadShowOnlySelected(_showOnlySelected);
  }

  void _toggleCheckboxVisibility() {
    setState(() {
      _showCheckboxes = !_showCheckboxes;
      if (!_showCheckboxes) {
        _clearSelection();
      }
    });
    context.read<AppState>().setDownloadShowCheckboxes(_showCheckboxes);
  }

  void _clearSelection() {
    setState(() {
      _selectedFullNames.clear();
      _showCheckboxes = false;
      _showOnlySelected = false;
    });
    final appState = context.read<AppState>();
    appState.setDownloadSelectedFullNames(_selectedFullNames);
    appState.setDownloadShowCheckboxes(false);
    appState.setDownloadShowOnlySelected(false);
  }

  void _install(String appFullName) {
    TaskRequest(
      task: TaskDownloadInstall(value: appFullName),
    ).sendSignalToRust();
  }

  void _download(String appFullName) {
    TaskRequest(
      task: TaskDownload(value: appFullName),
    ).sendSignalToRust();
  }

  Future<bool> _confirmDowngrades(
    BuildContext context,
    List<({InstalledPackage installed, CloudApp target})> items,
  ) async {
    if (items.isEmpty) return true;
    final l10n = AppLocalizations.of(context);
    final list = items
        .map((e) => l10n.downgradeItemFormat(e.installed.label,
            '${e.installed.versionCode}', '${e.target.versionCode}'))
        .join('\n');
    final res = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.downgradeAppsTitle),
        content: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(l10n.downgradeMultipleConfirmMessage),
              const SizedBox(height: 8),
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: Theme.of(context).colorScheme.surfaceContainer,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(
                    color: Theme.of(context).colorScheme.outlineVariant,
                  ),
                ),
                child: Text(list, softWrap: true),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            style: ButtonStyle(
              backgroundColor:
                  WidgetStatePropertyAll(Theme.of(context).colorScheme.error),
              foregroundColor:
                  WidgetStatePropertyAll(Theme.of(context).colorScheme.onError),
            ),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(l10n.commonConfirm),
          ),
        ],
      ),
    );
    return res ?? false;
  }

  Widget _buildSortButton(bool enabled) {
    final l10n = AppLocalizations.of(context);
    return PopupMenuButton<(SortOption, bool)>(
      enabled: enabled,
      tooltip: l10n.sortBy,
      icon: const Icon(Icons.sort),
      initialValue: (_sortOption, _sortAscending),
      itemBuilder: (context) => [
        PopupMenuItem(
          enabled: false,
          child: Text(l10n.sortBy),
        ),
        PopupMenuItem(
          value: (SortOption.name, true),
          child: Row(
            children: [
              Icon(_sortOption == SortOption.name && _sortAscending
                  ? Icons.radio_button_checked
                  : Icons.radio_button_unchecked),
              const SizedBox(width: 8),
              Text(l10n.sortNameAsc),
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
              Text(l10n.sortNameDesc),
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
              Text(l10n.sortDateOldest),
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
              Text(l10n.sortDateNewest),
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
              Text(l10n.sortSizeSmallest),
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
              Text(l10n.sortSizeLargest),
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
        // Persist sort state
        final sortKey = switch (_sortOption) {
          SortOption.name => 'name',
          SortOption.date => 'date',
          SortOption.size => 'size',
        };
        context.read<AppState>().setDownloadSort(sortKey, _sortAscending);
      },
    );
  }

  Widget _buildSearchButton() {
    if (_isSearching) {
      return SizedBox(
        width: 350,
        height: 40,
        child: Padding(
          padding: const EdgeInsets.only(right: 4.0),
          child: TextField(
            controller: _searchController,
            // autofocus: true,
            decoration: InputDecoration(
              hintText: AppLocalizations.of(context).searchAppsHint,
              contentPadding:
                  const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
              border: const OutlineInputBorder(),
              suffixIcon: _searchQuery.isNotEmpty
                  ? IconButton(
                      icon: const Icon(Icons.close),
                      onPressed: () {
                        _resetSearch();
                      },
                      tooltip: AppLocalizations.of(context).clearSearch,
                    )
                  : null,
            ),
            onChanged: (value) {
              _searchDebounceTimer?.cancel();
              _searchDebounceTimer =
                  Timer(const Duration(milliseconds: 300), () {
                setState(() {
                  _searchQuery = value;
                });
                // Persist search query
                context.read<AppState>().setDownloadSearchQuery(value);
              });
            },
          ),
        ),
      );
    }

    return IconButton(
      icon: const Icon(Icons.search),
      tooltip: AppLocalizations.of(context).search,
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
    // Persist scroll offset while scrolling
    _scrollController.addListener(() {
      if (_scrollController.hasClients) {
        context
            .read<AppState>()
            .setDownloadScrollOffset(_scrollController.position.pixels);
      }
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_initialized) return;
    final appState = context.read<AppState>();

    // Restore search query
    _searchQuery = appState.downloadSearchQuery;
    _searchController.text = _searchQuery;

    // Restore sort state
    final key = appState.downloadSortKey;
    _sortOption = switch (key) {
      'date' => SortOption.date,
      'size' => SortOption.size,
      _ => SortOption.name,
    };
    _sortAscending = appState.downloadSortAscending;

    // Restore selection / view prefs
    _showCheckboxes = appState.downloadShowCheckboxes;
    _showOnlySelected = appState.downloadShowOnlySelected;
    _selectedFullNames
      ..clear()
      ..addAll(appState.downloadSelectedFullNames);

    // Restore scroll position after layout
    final target = appState.downloadScrollOffset;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        final max = _scrollController.position.maxScrollExtent;
        _scrollController.jumpTo(target.clamp(0.0, max));
      }
    });

    _initialized = true;
  }

  Widget _buildFilterButton() {
    final hasSelections = _selectedFullNames.isNotEmpty;
    final l10n = AppLocalizations.of(context);

    return IconButton(
      icon: Icon(
        _showOnlySelected ? Icons.filter_list_off : Icons.filter_list,
        color: _showOnlySelected ? Theme.of(context).colorScheme.primary : null,
      ),
      tooltip: _showOnlySelected
          ? l10n.showAllItems
          : hasSelections
              ? l10n.showOnlySelectedItems
              : l10n.filterNoItems,
      onPressed: hasSelections ? _toggleShowOnlySelected : null,
    );
  }

  Widget _buildSelectionSummary(List<CachedAppData> allApps) {
    if (_selectedFullNames.isEmpty) return const SizedBox.shrink();

    final selectedApps = allApps
        .where((app) => _selectedFullNames.contains(app.app.fullName))
        .toList();
    final totalSize =
        selectedApps.fold<int>(0, (sum, app) => sum + app.app.size.toInt());
    final formattedTotalSize = _formatSize(totalSize);
    final l10n = AppLocalizations.of(context);

    return Container(
      padding: const EdgeInsets.all(16.0),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainer,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(12)),
      ),
      child: Row(
        children: [
          Text(
            l10n.selectedSummary(selectedApps.length, formattedTotalSize),
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
            label: Text(l10n.downloadSelected),
          ),
          const SizedBox(width: 8),
          Consumer<DeviceState>(
            builder: (context, deviceState, _) {
              return FilledButton.icon(
                onPressed: deviceState.isConnected
                    ? () async {
                        // Collect all downgrades first
                        final downgrades =
                            <({InstalledPackage installed, CloudApp target})>[];
                        for (final app in selectedApps) {
                          final installed =
                              deviceState.findInstalled(app.app.packageName);
                          if (installed != null &&
                              installed.versionCode.toInt() >
                                  app.app.versionCode) {
                            downgrades
                                .add((installed: installed, target: app.app));
                          }
                        }

                        final proceed =
                            await _confirmDowngrades(context, downgrades);
                        if (!proceed) return;

                        for (final app in selectedApps) {
                          _install(app.app.fullName);
                        }
                        _clearSelection();
                      }
                    : null,
                icon: const Icon(Icons.install_mobile),
                label: Text(l10n.installSelected),
              );
            },
          ),
          const SizedBox(width: 8),
          Consumer<SettingsState>(
            builder: (context, settings, _) {
              final names = selectedApps
                  .map((a) => a.app.originalPackageName)
                  .toList(growable: false);
              final favs = settings.favoritePackages;
              final allInFavorites =
                  names.isNotEmpty && names.every((pkg) => favs.contains(pkg));

              return FilledButton.icon(
                onPressed: () {
                  if (allInFavorites) {
                    settings.removeFavoritesBulk(names);
                  } else {
                    settings.addFavoritesBulk(names);
                  }
                  _clearSelection();
                },
                icon: Icon(allInFavorites ? Icons.star_outline : Icons.star),
                label: Text(allInFavorites
                    ? l10n.removeSelectedFromFavorites
                    : l10n.addSelectedToFavorites),
              );
            },
          ),
          const SizedBox(width: 8),
          IconButton(
            onPressed: _clearSelection,
            icon: const Icon(Icons.close),
            tooltip: l10n.clearSelection,
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final settingsState = context.watch<SettingsState>();
    return Consumer<CloudAppsState>(
      builder: (context, cloudAppsState, _) {
        final l10n = AppLocalizations.of(context);
        final showDownloaderInit = settingsState.isDownloaderInitializing;
        final showDownloaderError =
            !showDownloaderInit && settingsState.downloaderError != null;

        if (cloudAppsState.isLoading &&
            !showDownloaderInit &&
            !showDownloaderError) {
          return Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const CircularProgressIndicator(),
                const SizedBox(height: 12),
                Text(AppLocalizations.of(context).loadingApps),
              ],
            ),
          );
        }

        if (cloudAppsState.error != null &&
            !showDownloaderInit &&
            !showDownloaderError) {
          return Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  l10n.errorLoadingApps,
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const SizedBox(height: 8),
                Text(cloudAppsState.error!),
                const SizedBox(height: 16),
                FilledButton.icon(
                  onPressed: () => cloudAppsState.refresh(),
                  icon: const Icon(Icons.refresh),
                  label: Text(l10n.retry),
                ),
              ],
            ),
          );
        }

        var filteredAndSortedApps = _filterAndSortApps(cloudAppsState.apps);
        if (_showOnlyFavorites) {
          final favs = context.watch<SettingsState>().favoritePackages;
          filteredAndSortedApps = filteredAndSortedApps
              .where((a) => favs.contains(a.app.originalPackageName))
              .toList();
        }

        return Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                if (!showDownloaderInit && !showDownloaderError)
                  Padding(
                    padding: const EdgeInsets.all(16.0),
                    child: Row(
                      children: [
                        Text(
                          l10n.availableApps,
                          style: Theme.of(context).textTheme.titleLarge,
                        ),
                        const Spacer(),
                        if (!_showOnlySelected) _buildSearchButton(),
                        if (_showCheckboxes) _buildFilterButton(),
                        IconButton(
                          icon: Icon(
                            _showOnlyFavorites ? Icons.star : Icons.star_border,
                          ),
                          tooltip: _showOnlyFavorites
                              ? l10n.showAllItems
                              : l10n.showFavoritesOnly,
                          onPressed: _toggleShowOnlyFavorites,
                        ),
                        IconButton(
                          icon: Icon(_showCheckboxes
                              ? Icons.check_box
                              : Icons.check_box_outline_blank),
                          tooltip: l10n.multiSelect,
                          onPressed: _toggleCheckboxVisibility,
                        ),
                        // TODO: add search result sorting?
                        _buildSortButton(_searchQuery.isEmpty),
                        IconButton(
                          icon: const Icon(Icons.refresh),
                          tooltip: l10n.refresh,
                          onPressed: () => cloudAppsState.refresh(),
                        ),
                      ],
                    ),
                  ),
                if (showDownloaderInit)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                    child: _buildInitBanner(settingsState),
                  ),
                if (!showDownloaderInit && showDownloaderError)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                    child: _buildErrorBanner(settingsState.downloaderError!),
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
                          l10n.showingSelectedOnly,
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
                if (_showOnlyFavorites &&
                    !showDownloaderInit &&
                    !showDownloaderError)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                    child: Row(
                      children: [
                        Icon(
                          Icons.star,
                          size: 16,
                          color: Theme.of(context).colorScheme.tertiary,
                        ),
                        const SizedBox(width: 8),
                        Text(
                          l10n.showingFavoritesOnly,
                          style: Theme.of(context)
                              .textTheme
                              .bodyMedium
                              ?.copyWith(
                                color: Theme.of(context).colorScheme.tertiary,
                              ),
                        ),
                      ],
                    ),
                  ),
                if (!showDownloaderInit && !showDownloaderError)
                  Expanded(
                    child: CloudAppList(
                      apps: filteredAndSortedApps,
                      showCheckboxes: _showCheckboxes,
                      selectedFullNames: _selectedFullNames,
                      scrollController: _scrollController,
                      isSearching: _searchQuery.isNotEmpty,
                      onSelectionChanged: _toggleSelection,
                      onDownload: _download,
                      onInstall: _install,
                    ),
                  ),
                if (!showDownloaderInit && !showDownloaderError)
                  _buildSelectionSummary(_sortApps(cloudAppsState.apps)),
              ],
            ),
          ),
        );
      },
    );
  }

  Widget _buildInitBanner(SettingsState s) {
    final l10n = AppLocalizations.of(context);
    final progress = s.downloaderInitProgress;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12.0),
        child: Row(
          children: [
            const SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(strokeWidth: 2)),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(l10n.preparingDownloader),
                  const SizedBox(height: 4),
                  LinearProgressIndicator(value: progress),
                  const SizedBox(height: 4),
                  Text(l10n.downloadingRcloneFiles,
                      style: const TextStyle(fontSize: 12)),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildErrorBanner(String error) {
    return Card(
      color: Colors.red.withValues(alpha: 0.08),
      child: Padding(
        padding: const EdgeInsets.all(12.0),
        child: Row(
          children: [
            const Icon(Icons.error_outline, color: Colors.red),
            const SizedBox(width: 8),
            Expanded(child: Text(error)),
            const SizedBox(width: 12),
            FilledButton.tonalIcon(
              onPressed: () {
                const RetryDownloaderInitRequest().sendSignalToRust();
              },
              icon: const Icon(Icons.refresh),
              label: const Text('Retry'),
            ),
          ],
        ),
      ),
    );
  }
}

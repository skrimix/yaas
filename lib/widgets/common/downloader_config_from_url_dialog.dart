import 'dart:async';
import 'dart:io';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:rinf/rinf.dart';

import '../../src/bindings/bindings.dart';
import '../../src/l10n/app_localizations.dart';

const _kDownloaderTemplateVrpId = 'vrp-public';
const _kDownloaderTemplateVrgRusId = 'vrg-rus';
const _kDownloaderTemplateNifId = 'nif';
const _kDownloaderTemplateCustomId = 'custom';

const _kDownloaderTemplateVrpUrl =
    'https://github.com/skrimix/yaas/releases/download/files/downloader_vrp.json';
const _kDownloaderTemplateVrgRusUrl =
    'https://qloader.5698452.xyz/files/rclone/config/downloader_ru.json';
const _kDownloaderTemplateNifUrl =
    'https://qloader.5698452.xyz/files/rclone/config/downloader_nif.json';

class DownloaderConfigFromUrlDialog extends StatefulWidget {
  const DownloaderConfigFromUrlDialog({
    super.key,
    this.initialConfigId,
  });

  final String? initialConfigId;

  @override
  State<DownloaderConfigFromUrlDialog> createState() =>
      _DownloaderConfigFromUrlDialogState();
}

class _DownloaderConfigFromUrlDialogState
    extends State<DownloaderConfigFromUrlDialog> {
  late String _selectedTemplateId;
  final TextEditingController _urlController = TextEditingController();
  StreamSubscription<RustSignalPack<DownloaderConfigInstallResult>>?
      _installSub;
  bool _isInstalling = false;
  String? _errorText;
  _VrgRusTestState _vrgRusTestState = _VrgRusTestState.idle;
  String? _vrgRusErrorMessage;
  int? _vrgRusErrorMessageCode;

  @override
  void initState() {
    super.initState();
    _selectedTemplateId = _inferInitialTemplateId(widget.initialConfigId);
    _urlController.text = _urlForTemplate(_selectedTemplateId);
    _installSub =
        DownloaderConfigInstallResult.rustSignalStream.listen(_onInstallResult);
  }

  @override
  void dispose() {
    _installSub?.cancel();
    _urlController.dispose();
    super.dispose();
  }

  String _inferInitialTemplateId(String? currentId) {
    switch (currentId) {
      case _kDownloaderTemplateVrpId:
      case _kDownloaderTemplateVrgRusId:
      case _kDownloaderTemplateNifId:
        return currentId!;
      default:
        return _kDownloaderTemplateVrpId;
    }
  }

  String _urlForTemplate(String templateId) {
    switch (templateId) {
      case _kDownloaderTemplateVrpId:
        return _kDownloaderTemplateVrpUrl;
      case _kDownloaderTemplateVrgRusId:
        return _kDownloaderTemplateVrgRusUrl;
      case _kDownloaderTemplateNifId:
        return _kDownloaderTemplateNifUrl;
      case _kDownloaderTemplateCustomId:
        return _urlController.text;
      default:
        return _kDownloaderTemplateVrpUrl;
    }
  }

  void _onTemplateChanged(String? templateId) {
    if (templateId == null) return;
    setState(() {
      _selectedTemplateId = templateId;
      _errorText = null;
      // _vrgRusTestState = _VrgRusTestState.idle;
      if (templateId != _kDownloaderTemplateCustomId) {
        _urlController.text = _urlForTemplate(templateId);
      }
    });
  }

  bool _isValidUrl(String value) {
    final url = value.trim();
    if (url.isEmpty) return false;
    final lower = url.toLowerCase();
    if (!lower.startsWith('http://') && !lower.startsWith('https://')) {
      return false;
    }
    return Uri.tryParse(url)?.hasAbsolutePath ?? false;
  }

  void _onInstallPressed() {
    final l10n = AppLocalizations.of(context);
    final url = _urlController.text.trim();
    if (!_isValidUrl(url)) {
      setState(() {
        _errorText = l10n.downloaderConfigUrlInvalid;
      });
      return;
    }
    setState(() {
      _isInstalling = true;
      _errorText = null;
    });
    InstallDownloaderConfigFromUrlRequest(url: url).sendSignalToRust();
  }

  bool get _needsVrgRusTest =>
      _selectedTemplateId == _kDownloaderTemplateVrgRusId;

  bool get _vrgRusTestCompleted => _vrgRusTestState == _VrgRusTestState.success;
  // _vrgRusTestState == _VrgRusTestState.noAccess;

  Future<void> _testVrgRusAccess() async {
    if (_vrgRusTestState == _VrgRusTestState.inProgress) return;
    setState(() {
      _vrgRusTestState = _VrgRusTestState.inProgress;
      _vrgRusErrorMessage = null;
    });
    try {
      final client = HttpClient()
        ..connectionTimeout = const Duration(seconds: 15);
      try {
        final request = await client.getUrl(
          Uri.parse('https://yaas.dipvr.ru:9227/api/v1/verification'),
        );
        final response = await request.close();
        final body = await response.transform(utf8.decoder).join();

        if (!mounted) {
          client.close(force: true);
          return;
        }

        if (response.statusCode == 200) {
          setState(() {
            _vrgRusTestState = _VrgRusTestState.success;
            _vrgRusErrorMessage = null;
          });
        } else if (response.statusCode == 403) {
          setState(() {
            _vrgRusTestState = _VrgRusTestState.noAccess;
            _vrgRusErrorMessage = body.trim().isNotEmpty ? body.trim() : null;
            _vrgRusErrorMessageCode = response.statusCode;
          });
        } else {
          setState(() {
            _vrgRusTestState = _VrgRusTestState.noAccess;
            _vrgRusErrorMessage = body.trim().isNotEmpty ? body.trim() : null;
            _vrgRusErrorMessageCode = response.statusCode;
          });
        }
      } finally {
        client.close();
      }
    } catch (e) {
      debugPrint('Error testing VRG Rus access: $e');
      if (!mounted) return;
      setState(() {
        _vrgRusTestState = _VrgRusTestState.noAccess;
        _vrgRusErrorMessage = e.toString();
        _vrgRusErrorMessageCode = 0;
      });
    }
  }

  void _onInstallResult(
    RustSignalPack<DownloaderConfigInstallResult> pack,
  ) {
    if (!_isInstalling) {
      return;
    }
    final result = pack.message;
    if (!mounted) return;

    if (result.success) {
      setState(() {
        _isInstalling = false;
      });
      Navigator.of(context).pop();
    } else {
      final l10n = AppLocalizations.of(context);
      setState(() {
        _isInstalling = false;
        _errorText = result.error?.isNotEmpty == true
            ? result.error
            : l10n.downloaderConfigInstallFailed;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final canEditUrl = _selectedTemplateId == _kDownloaderTemplateCustomId;
    final isValid = _isValidUrl(_urlController.text);
    final needsVrgTest = _needsVrgRusTest;
    final canInstall =
        !_isInstalling && isValid && (!needsVrgTest || _vrgRusTestCompleted);

    return AlertDialog(
      title: Text(l10n.downloaderConfigFromUrlTitle),
      content: SizedBox(
        width: 480,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              l10n.downloaderConfigFromUrlDescription,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: 12),
            RadioGroup<String>(
              groupValue: _selectedTemplateId,
              onChanged: (value) {
                if (_isInstalling) return;
                _onTemplateChanged(value);
              },
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  RadioListTile<String>(
                    value: _kDownloaderTemplateVrpId,
                    title: Text(l10n.downloaderConfigTemplateVrp),
                    subtitle: Text(
                      l10n.downloaderConfigTemplateVrpHint,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Theme.of(context).colorScheme.secondary),
                    ),
                    dense: true,
                  ),
                  RadioListTile<String>(
                    value: _kDownloaderTemplateVrgRusId,
                    title: Text(l10n.downloaderConfigTemplateVrgRus),
                    subtitle: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          l10n.downloaderConfigTemplateVrgRusHint,
                          style: Theme.of(context)
                              .textTheme
                              .bodySmall
                              ?.copyWith(
                                  color:
                                      Theme.of(context).colorScheme.secondary),
                        ),
                        const SizedBox(height: 4),
                        Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            TextButton.icon(
                              onPressed: _isInstalling ||
                                      _vrgRusTestState ==
                                          _VrgRusTestState.inProgress
                                  ? null
                                  : _testVrgRusAccess,
                              icon: const Icon(Icons.wifi_tethering),
                              label: Text(
                                l10n.downloaderConfigVrgRusTestButton,
                              ),
                            ),
                            if (_vrgRusTestState ==
                                _VrgRusTestState.inProgress) ...[
                              const SizedBox(width: 8),
                              const SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              ),
                            ],
                          ],
                        ),
                        if (_vrgRusTestState == _VrgRusTestState.success) ...[
                          const SizedBox(height: 2),
                          Text(
                            l10n.downloaderConfigVrgRusTestOk,
                            style:
                                Theme.of(context).textTheme.bodySmall?.copyWith(
                                      color: Colors.green.shade300,
                                    ),
                          ),
                        ] else if (_vrgRusTestState ==
                            _VrgRusTestState.noAccess) ...[
                          const SizedBox(height: 2),
                          Text(
                            l10n.downloaderConfigVrgRusTestError(
                              _vrgRusErrorMessageCode ?? 0,
                              _vrgRusErrorMessage ?? '',
                            ),
                            style: Theme.of(context)
                                .textTheme
                                .bodySmall
                                ?.copyWith(
                                  color: Theme.of(context).colorScheme.error,
                                ),
                          ),
                        ],
                      ],
                    ),
                    dense: true,
                  ),
                  RadioListTile<String>(
                    value: _kDownloaderTemplateNifId,
                    title: Text(l10n.downloaderConfigTemplateNif),
                    subtitle: Text(
                      l10n.downloaderConfigTemplateNifHint,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: Theme.of(context).colorScheme.secondary),
                    ),
                    dense: true,
                  ),
                  RadioListTile<String>(
                    value: _kDownloaderTemplateCustomId,
                    title: Text(l10n.downloaderConfigTemplateCustom),
                    dense: true,
                  ),
                ],
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: _urlController,
              enabled: !_isInstalling && canEditUrl,
              decoration: InputDecoration(
                labelText: l10n.downloaderConfigUrlLabel,
                border: const OutlineInputBorder(),
                errorText: _errorText,
              ),
              onChanged: (_) {
                if (_errorText != null) {
                  setState(() {
                    _errorText = null;
                  });
                }
              },
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(_isInstalling ? l10n.commonClose : l10n.commonCancel),
        ),
        Tooltip(
          message: (!canInstall && needsVrgTest && !_vrgRusTestCompleted)
              ? l10n.downloaderConfigVrgRusTestRequiredTooltip
              : '',
          child: FilledButton(
            onPressed: canInstall ? _onInstallPressed : null,
            child: _isInstalling
                ? Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                      const SizedBox(width: 8),
                      Text(l10n.downloaderConfigInstalling),
                    ],
                  )
                : Text(l10n.downloaderConfigInstallButton),
          ),
        ),
      ],
    );
  }
}

enum _VrgRusTestState {
  idle,
  inProgress,
  success,
  noAccess,
}

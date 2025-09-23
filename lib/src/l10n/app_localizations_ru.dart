// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Russian (`ru`).
class AppLocalizationsRu extends AppLocalizations {
  AppLocalizationsRu([String locale = 'ru']) : super(locale);

  @override
  String get appTitle => 'YAAS';

  @override
  String get navHome => 'Главная';

  @override
  String get navManage => 'Управление';

  @override
  String get navDownload => 'Загрузка';

  @override
  String get navSideload => 'Установка';

  @override
  String get navBackups => 'Бэкапы';

  @override
  String get navSettings => 'Настройки';

  @override
  String get navLogs => 'Журнал';

  @override
  String get navAbout => 'О программе';

  @override
  String get navDownloads => 'Загрузки';

  @override
  String get settingsTitle => 'Настройки';

  @override
  String get settingsErrorLoading => 'Ошибка загрузки настроек';

  @override
  String get settingsResetToDefaults => 'Сбросить по умолчанию';

  @override
  String get settingsRevertChangesTooltip =>
      'Отменить изменения\n(Shift+Клик для полного сброса)';

  @override
  String get settingsSaveChanges => 'Сохранить изменения';

  @override
  String get settingsSectionGeneral => 'Общие';

  @override
  String get settingsLanguage => 'Язык';

  @override
  String get settingsNavigationRailLabels => 'Подписи панели навигации';

  @override
  String get settingsNavigationRailLabelsSelected =>
      'Только для выбранной страницы';

  @override
  String get settingsNavigationRailLabelsAll => 'Для всех страниц';

  @override
  String get settingsStartupPage => 'Стартовая страница';

  @override
  String get settingsSystemDefault => 'Системный';

  @override
  String get languageEnglish => 'Английский';

  @override
  String get languageRussian => 'Русский';

  @override
  String get settingsSectionStorage => 'Хранилище';

  @override
  String get settingsDownloadsLocation => 'Папка загрузок';

  @override
  String get settingsBackupsLocation => 'Папка резервных копий';

  @override
  String get settingsSectionAdb => 'ADB';

  @override
  String get settingsAdbPath => 'Путь к ADB';

  @override
  String get settingsPreferredConnection => 'Предпочтительный тип подключения';

  @override
  String get settingsConnectionUsb => 'USB';

  @override
  String get settingsConnectionWireless => 'Беспроводное';

  @override
  String get settingsSectionDownloader => 'Загрузчик';

  @override
  String get settingsRclonePath => 'Путь к Rclone';

  @override
  String get settingsRcloneRemote => 'Rclone хранилище';

  @override
  String get settingsCustomRemoteName => 'Другое имя хранилища';

  @override
  String get settingsCustomInput => '[Другое]';

  @override
  String get settingsNoRemotesFound => 'Хранилища не найдены';

  @override
  String get settingsFailedToLoadRemotes => 'Не удалось перечислить хранилища';

  @override
  String get settingsBandwidthLimit => 'Ограничение скорости';

  @override
  String get settingsBandwidthHelper =>
      '(В разработке) Значение в КиБ/с или с суффиксами B|K|M|G|T|P и др. (нажмите для справки)';

  @override
  String get settingsDownloadsCleanup => 'Очистка загрузок';

  @override
  String get settingsCleanupDeleteAfterInstall => 'Удалять после установки';

  @override
  String get settingsCleanupKeepOneVersion => 'Хранить одну версию';

  @override
  String get settingsCleanupKeepTwoVersions => 'Хранить две версии';

  @override
  String get settingsCleanupKeepAllVersions => 'Хранить все версии';

  @override
  String get settingsBrowse => 'Обзор';

  @override
  String selectLabel(String label) {
    return 'Выберите: $label';
  }

  @override
  String selectLabelDirectory(String label) {
    return 'Выберите папку: $label';
  }

  @override
  String couldNotOpenUrl(String url) {
    return 'Не удалось открыть $url';
  }

  @override
  String statusAdb(String status) {
    return 'Статус ADB: $status';
  }

  @override
  String get statusAdbServerNotRunning => 'ADB сервер не запущен';

  @override
  String get statusAdbServerStarting => 'Запуск ADB сервера';

  @override
  String get statusAdbNoDevices => 'Нет найденных устройств';

  @override
  String statusAdbDevicesAvailable(int count) {
    return 'Доступны устройства ($count)';
  }

  @override
  String get statusAdbConnected => 'Устройство подключено';

  @override
  String get statusAdbDeviceUnauthorized => 'Устройство не авторизовано';

  @override
  String get statusAdbUnknown => 'Неизвестно';

  @override
  String statusDeviceInfo(String name, String serial) {
    return 'Устройство: $name\nСерийный номер: $serial';
  }

  @override
  String storageTooltip(String available, String total) {
    return '$available свободно из $total';
  }

  @override
  String activeTasks(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count активных задач',
      many: '$count активных задач',
      few: '$count активные задачи',
      one: '$count активная задача',
    );
    return '$_temp0';
  }

  @override
  String get viewTasks => 'Задачи';

  @override
  String get refreshAllData => 'Обновить все данные';

  @override
  String get noDeviceConnected => 'Устройство не подключено';

  @override
  String get dragDropDropToInstall => 'Перетащите для установки/восстановления';

  @override
  String get dragDropNoDevice => 'Устройство не подключено';

  @override
  String get dragDropHintConnected =>
      'Перетащите APK/директорию приложения для установки или папку бэкапа для восстановления';

  @override
  String get dragDropHintDisconnected =>
      'Подключите устройство, чтобы использовать перетаскивание';

  @override
  String get dragDropInvalidDir =>
      'Папка не является корректной директорией приложения или бэкапом';

  @override
  String get dragDropInvalidFile => 'Файл не является корректным APK';

  @override
  String get connectDeviceToInstall =>
      'Подключите устройство, чтобы устанавливать приложения';

  @override
  String get batteryDumpCopied =>
      'Состояние батареи скопировано в буфер обмена';

  @override
  String get batteryDumpFailed => 'Не удалось получить состояние батареи';

  @override
  String get commonSuccess => 'Успешно';

  @override
  String get commonError => 'Ошибка';

  @override
  String get diagnosticsTitle => 'Диагностика подключения';

  @override
  String get diagnosticsAdbServer => 'Сервер ADB';

  @override
  String get diagnosticsDevices => 'Устройства';

  @override
  String get diagnosticsAuthorization => 'Авторизация';

  @override
  String get diagnosticsActiveDevice => 'Активное устройство';

  @override
  String get diagnosticsAdbPath => 'Путь к ADB';

  @override
  String get diagnosticsServerNotRunningDesc =>
      'Сервер ADB не запущен. Убедитесь, что ADB установлен и доступен в PATH, либо укажите путь к ADB в настройках.';

  @override
  String get diagnosticsServerStartingDesc => 'Запуск сервера ADB...';

  @override
  String get diagnosticsServerRunningDesc => 'Сервер ADB запущен.';

  @override
  String get diagnosticsNoDevicesDesc =>
      'Устройства не обнаружены. Включите ADB/режим разработчика и подключите устройство по USB.';

  @override
  String diagnosticsDevicesAvailableDesc(int count) {
    return 'Обнаружено устройств ($count)';
  }

  @override
  String get diagnosticsUnauthorizedDesc =>
      'Устройство не авторизовано. Подтвердите запрос авторизации на устройстве.';

  @override
  String get diagnosticsAuthorizedDesc => 'Устройство авторизовано.';

  @override
  String get diagnosticsConnectedDesc => 'Устройство подключено и готово.';

  @override
  String get diagnosticsUnknownDesc => 'Неизвестное состояние.';

  @override
  String diagnosticsConfiguredPath(String path) {
    return 'Указанный путь: $path';
  }

  @override
  String get diagnosticsUsingSystemPath => 'Используется системный PATH';

  @override
  String get commonYes => 'Да';

  @override
  String get commonNo => 'Нет';

  @override
  String get deviceActions => 'Действия с устройством';

  @override
  String get deviceProximitySensor => 'Датчик приближения';

  @override
  String get disableProximitySensor => 'Отключить датчик приближения';

  @override
  String get enableProximitySensor => 'Включить датчик приближения';

  @override
  String get deviceGuardian => 'Guardian';

  @override
  String get guardianSuspend => 'Приостановить Guardian';

  @override
  String get guardianResume => 'Возобновить Guardian';

  @override
  String get copiedToClipboard => 'Скопировано в буфер обмена';

  @override
  String get clickToCopy => 'Нажмите, чтобы скопировать';

  @override
  String get detailsPackageName => 'Имя пакета:';

  @override
  String get detailsVersion => 'Версия:';

  @override
  String get detailsVersionCode => 'Код версии:';

  @override
  String get detailsIsVr => 'VR-приложение:';

  @override
  String get detailsIsLaunchable => 'Запускаемое:';

  @override
  String get detailsIsSystem => 'Системное:';

  @override
  String get detailsStorageUsage => 'Использование хранилища:';

  @override
  String get detailsApp => 'Приложение:';

  @override
  String get detailsData => 'Данные:';

  @override
  String get detailsCache => 'Кэш:';

  @override
  String get detailsTotal => 'Итого:';

  @override
  String get detailsRating => 'Рейтинг:';

  @override
  String get detailsReviewsTitle => 'Недавние отзывы';

  @override
  String get detailsReviewsUnavailable =>
      'Отзывы недоступны для этого приложения.';

  @override
  String get detailsReviewsError => 'Не удалось загрузить отзывы.';

  @override
  String get detailsReviewsEmpty => 'Отзывов пока нет.';

  @override
  String get commonCopy => 'Копировать';

  @override
  String get commonClose => 'Закрыть';

  @override
  String get commonCancel => 'Отмена';

  @override
  String get availableVersions => 'Доступные версии';

  @override
  String get installNewerVersion => 'Установить более новую версию';

  @override
  String get reinstallThisVersion => 'Переустановить эту версию';

  @override
  String get holdShiftToReinstall => 'Удерживайте Shift для переустановки';

  @override
  String get cannotDowngrade => 'Нельзя откатиться на более старую версию';

  @override
  String get newerVersion => 'Более новая версия';

  @override
  String get sameVersion => 'Эта же версия';

  @override
  String get olderVersion => 'Более старая версия';

  @override
  String get update => 'Обновить';

  @override
  String get install => 'Установить';

  @override
  String get checkForUpdates => 'Проверить обновления';

  @override
  String get noMatchingCloudApp =>
      'Нет подходящего приложения в облачном репозитории';

  @override
  String updateFromTo(String from, String to) {
    return 'Обновить с $from до $to';
  }

  @override
  String get noAppsInCategory => 'Нет приложений в этой категории';

  @override
  String get appDetails => 'Сведения о приложении';

  @override
  String get launch => 'Запустить';

  @override
  String get forceStop => 'Принудительно остановить';

  @override
  String get backupApp => 'Резервное копирование';

  @override
  String get backup => 'Резервная копия';

  @override
  String get uninstall => 'Удалить';

  @override
  String segmentVrApps(int count) {
    return 'VR-приложения ($count)';
  }

  @override
  String segmentOtherApps(int count) {
    return 'Другие приложения ($count)';
  }

  @override
  String segmentSystemApps(int count) {
    return 'Системные и скрытые ($count)';
  }

  @override
  String get noAppsFound => 'Приложений не найдено';

  @override
  String get noAppsAvailable => 'Нет доступных приложений';

  @override
  String get copyFullName => 'Скопировать полное имя';

  @override
  String get copyPackageName => 'Скопировать имя пакета';

  @override
  String sizeAndDate(String size, String date) {
    return 'Размер: $size • Обновлено: $date';
  }

  @override
  String get downloadToComputer => 'Скачать на компьютер';

  @override
  String get downloadAndInstall => 'Скачать и установить на устройство';

  @override
  String get downloadAndInstallNotConnected =>
      'Скачать и установить на устройство (не подключено)';

  @override
  String get sortBy => 'Сортировать по';

  @override
  String get sortNameAsc => 'Имя (A → Z)';

  @override
  String get sortNameDesc => 'Имя (Z → A)';

  @override
  String get sortDateOldest => 'Дата (сначала старые)';

  @override
  String get sortDateNewest => 'Дата (сначала новые)';

  @override
  String get sortSizeSmallest => 'Размер (сначала меньшие)';

  @override
  String get sortSizeLargest => 'Размер (сначала большие)';

  @override
  String get searchAppsHint => 'Поиск приложений...';

  @override
  String get clearSearch => 'Очистить поиск';

  @override
  String get search => 'Поиск';

  @override
  String get showAllItems => 'Показать все';

  @override
  String get showOnlySelectedItems => 'Показать только выбранные';

  @override
  String get filterNoItems => 'Фильтр (ничего не выбрано)';

  @override
  String selectedSummary(int count, String total) {
    return 'Выбрано: $count • Итого: $total';
  }

  @override
  String get downloadSelected => 'Скачать выбранные';

  @override
  String get installSelected => 'Установить выбранные';

  @override
  String get clearSelection => 'Очистить выбор';

  @override
  String get errorLoadingApps => 'Ошибка загрузки списка';

  @override
  String get retry => 'Повторить';

  @override
  String get availableApps => 'Доступные приложения';

  @override
  String get underConstruction => 'В разработке';

  @override
  String get multiSelect => 'Множественный выбор';

  @override
  String get refresh => 'Обновить';

  @override
  String get showingSelectedOnly => 'Показаны только выбранные';

  @override
  String get deviceTitle => 'Устройство';

  @override
  String get leftController => 'Левый контроллер';

  @override
  String get rightController => 'Правый контроллер';

  @override
  String get headset => 'Шлем';

  @override
  String get deviceActionsTooltip => 'Действия с устройством';

  @override
  String get statusLabel => 'Статус';

  @override
  String get batteryLabel => 'Батарея';

  @override
  String get powerOffDevice => 'Выключить устройство';

  @override
  String get powerOffConfirm => 'Вы уверены, что хотите выключить устройство?';

  @override
  String get powerOffMenu => 'Выключить...';

  @override
  String get rebootMenu => 'Перезагрузить...';

  @override
  String get rebootOptions => 'Параметры перезагрузки';

  @override
  String get rebootNormal => 'Обычная';

  @override
  String get rebootBootloader => 'Загрузчик';

  @override
  String get rebootRecovery => 'Recovery';

  @override
  String get rebootFastboot => 'Fastboot';

  @override
  String get rebootDevice => 'Перезагрузить устройство';

  @override
  String get rebootNowConfirm => 'Перезагрузить устройство сейчас?';

  @override
  String get rebootToBootloader => 'Перезагрузить в загрузчик';

  @override
  String get rebootToBootloaderConfirm =>
      'Перезагрузить устройство в загрузчик?';

  @override
  String get rebootToRecovery => 'Перезагрузить в recovery';

  @override
  String get rebootToRecoveryConfirm => 'Перезагрузить устройство в recovery?';

  @override
  String get rebootToFastboot => 'Перезагрузить в fastboot';

  @override
  String get rebootToFastbootConfirm => 'Перезагрузить устройство в fastboot?';

  @override
  String get commonConfirm => 'Подтвердить';

  @override
  String get delete => 'Удалить';

  @override
  String get restore => 'Восстановить';

  @override
  String get mute => 'Выключить звук';

  @override
  String get unmute => 'Включить звук';

  @override
  String get close => 'Закрыть';

  @override
  String get pause => 'Пауза';

  @override
  String get checkingTrailerAvailability => 'Проверка доступности трейлера...';

  @override
  String get trailerAvailable => 'Трейлер доступен';

  @override
  String get noTrailer => 'Трейлер не доступен';

  @override
  String get backupsTitle => 'Резервные копии';

  @override
  String get openBackupsFolder => 'Открыть папку резервных копий';

  @override
  String get openDownloadsFolder => 'Открыть папку загрузок';

  @override
  String get downloadsTitle => 'Загрузки';

  @override
  String get deleteAllDownloads => 'Удалить все загрузки';

  @override
  String get deleteAllDownloadsTitle => 'Удалить все загрузки';

  @override
  String get deleteAllDownloadsConfirm =>
      'Вы уверены, что хотите удалить все загрузки?';

  @override
  String deleteAllDownloadsResult(String removed, String skipped) {
    return 'Удалено $removed, пропущено $skipped';
  }

  @override
  String get deleteDownloadTitle => 'Удалить загрузку';

  @override
  String deleteDownloadConfirm(String name) {
    return 'Удалить \"$name\"?';
  }

  @override
  String get downloadDeletedTitle => 'Загрузка удалена';

  @override
  String get noBackupsFound => 'Резервные копии не найдены.';

  @override
  String get noDownloadsFound => 'Загрузки не найдены.';

  @override
  String get unsupportedPlatform => 'Платформа не поддерживается';

  @override
  String get folderPathCopied => 'Путь к папке скопирован в буфер обмена';

  @override
  String unableToOpenFolder(String path) {
    return 'Не удалось открыть папку: $path';
  }

  @override
  String get openFolderTooltip => 'Открыть папку';

  @override
  String get unknownTime => 'Неизвестное время';

  @override
  String get partAPK => 'APK';

  @override
  String get partPrivate => 'Приватные';

  @override
  String get partShared => 'Общие';

  @override
  String get partOBB => 'OBB';

  @override
  String get noPartsDetected => 'Компоненты не обнаружены';

  @override
  String get deleteBackupTitle => 'Удалить резервную копию';

  @override
  String deleteBackupConfirm(String name) {
    return 'Удалить \"$name\"?';
  }

  @override
  String get backupDeletedTitle => 'Резервная копия удалена';

  @override
  String get fatalErrorTitle => 'Критическая ошибка';

  @override
  String get exitApplication => 'Выход из приложения';

  @override
  String get errorCopied => 'Сообщение об ошибке скопировано';

  @override
  String get copyError => 'Копировать ошибку';

  @override
  String get selectAppDirectoryTitle => 'Выберите папку приложения';

  @override
  String get selectApkFileTitle => 'Выберите файл APK';

  @override
  String get selectedInvalidDir =>
      'Выбранный путь не является директорией приложения';

  @override
  String get selectedInvalidApk => 'Выбранный путь не является корректным APK';

  @override
  String get singleApk => 'Один APK';

  @override
  String get appDirectory => 'Папка приложения';

  @override
  String get appDirectoryPath => 'Путь к папке приложения';

  @override
  String get apkFilePath => 'Путь к файлу APK';

  @override
  String get pathHintDirectory =>
      'Выберите или введите путь к папке приложения';

  @override
  String get pathHintApk => 'Выберите или введите путь к файлу APK';

  @override
  String get directoryRequirements =>
      'Директория должна содержать APK и при необходимости папку данных OBB или файл install.txt.';

  @override
  String get proTipDragDrop =>
      'Подсказка: можно перетащить APK или папку приложения в любое место приложения для установки.';

  @override
  String get addedToQueue => 'Добавлено в очередь!';

  @override
  String get sideloadApp => 'Установить из папки';

  @override
  String get installApk => 'Установить APK';

  @override
  String get tasksTitle => 'Задачи';

  @override
  String get tasksTabActive => 'Активные';

  @override
  String get tasksTabRecent => 'Недавние';

  @override
  String get tasksEmptyActive => 'Нет активных задач';

  @override
  String get tasksEmptyRecent => 'Нет недавних задач';

  @override
  String get cancelTask => 'Отменить задачу';

  @override
  String get taskTypeDownload => 'Скачать';

  @override
  String get taskTypeDownloadInstall => 'Скачать и установить';

  @override
  String get taskTypeInstallApk => 'Установить APK';

  @override
  String get taskTypeInstallLocalApp => 'Установить из папки';

  @override
  String get taskTypeUninstall => 'Удалить';

  @override
  String get taskTypeBackupApp => 'Резервное копирование';

  @override
  String get taskTypeRestoreBackup => 'Восстановить из копии';

  @override
  String get taskStatusWaiting => 'Ожидание';

  @override
  String get taskStatusRunning => 'Выполняется';

  @override
  String get taskStatusCompleted => 'Завершено';

  @override
  String get taskStatusFailed => 'Ошибка';

  @override
  String get taskStatusCancelled => 'Отменено';

  @override
  String get taskUnknown => 'Неизвестно';

  @override
  String get backupOptionsTitle => 'Параметры резервного копирования';

  @override
  String get backupSelectParts => 'Выберите части для копирования:';

  @override
  String get backupAppData => 'Данные приложения';

  @override
  String get backupApk => 'APK';

  @override
  String get backupObbFiles => 'Файлы OBB';

  @override
  String get backupNameSuffix => 'Суффикс имени (необязательно)';

  @override
  String get backupNameSuffixHint => 'например: перед обновлением';

  @override
  String get startBackup => 'Начать копирование';

  @override
  String get logsSearchTooltip =>
      'Ищите по уровню, сообщению, цели или ID промежутка. Примеры: \"error\", \"info\", \"adb\", \"connect\", \"13\"';

  @override
  String get logsSearchHint => 'Поиск в логах...';

  @override
  String get clearCurrentLogs => 'Очистить текущие логи';

  @override
  String get exportLogs => 'Экспорт логов';

  @override
  String get openLogsDirectory => 'Открыть папку логов';

  @override
  String get clearFilters => 'Сбросить фильтры';

  @override
  String get noLogsToDisplay => 'Нет логов для отображения';

  @override
  String get logsAppearHere => 'Сообщения логов будут появляться здесь';

  @override
  String get logEntryCopied => 'Запись лога скопирована';

  @override
  String get spanId => 'ID промежутка';

  @override
  String get filterBySpanId => 'Фильтровать логи по этому ID промежутка';

  @override
  String get spanTrace => 'Трейс промежутков:';

  @override
  String get spansLabel => 'Промежутки';

  @override
  String get logsSpanEventsTooltip =>
      'Показывать/скрывать события создания и уничтожения промежутков. Промежутки отслеживают поток выполнения.';

  @override
  String logsOpenNotSupported(String path) {
    return 'Платформа не поддерживается. Путь к папке логов скопирован: $path';
  }

  @override
  String logsOpenFailed(String path) {
    return 'Не удалось открыть папку логов (скопировано): $path';
  }

  @override
  String get createdWord => 'создан';

  @override
  String get closedWord => 'закрыт';

  @override
  String get noMessage => 'нет сообщения';

  @override
  String get uninstallAppTitle => 'Удалить приложение';

  @override
  String uninstallConfirmMessage(String app) {
    return 'Удалить \"$app\"?\n\nЭто действие удалит приложение и все данные.';
  }

  @override
  String get uninstalledDone => 'Удалено!';

  @override
  String get uninstalling => 'Удаление...';

  @override
  String get clearLogsTitle => 'Очистить логи';

  @override
  String get clearLogsMessage =>
      'Это действие очистит текущие логи. Его нельзя отменить.';

  @override
  String get commonClear => 'Очистить';

  @override
  String logsCopied(int count) {
    return 'Скопировано логов: $count';
  }

  @override
  String get emptyValue => '(пусто)';

  @override
  String get errorWord => 'ошибка';
}

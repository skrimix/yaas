import 'package:yaas/providers/task_state.dart';
import 'package:yaas/src/bindings/bindings.dart';

Uint64 uint64(int value) => Uint64.fromBigInt(BigInt.from(value));

final apps = [
  for (final (index, name, slug, size) in [
    (0, 'Aurora Trails', 'aurora', 1240000000),
    (1, 'Canvas Studio', 'canvas', 680000000),
    (2, 'Gravity Garden', 'gravity', 2350000000),
    (3, 'Orbit Workshop', 'orbit', 1840000000),
    (4, 'Pocket Observatory', 'observatory', 920000000),
    (5, 'Rhythm Harbor', 'rhythm', 3150000000),
    (6, 'Summit Explorer', 'summit', 4280000000),
    (7, 'Tabletop Islands', 'tabletop', 1560000000),
  ])
    CloudApp(
      appName: name,
      fullName: '$name v${120 + index}+1.2.${index + 1}',
      packageName: 'org.example.$slug',
      truePackageName: 'org.example.$slug',
      versionCode: 120 + index,
      lastUpdated: '2026-09-0${7 - index % 7} 12:00',
      size: uint64(size),
      popularity:
          Popularity(day1: 4 + index, day7: 8 + index, day30: 12 + index),
    ),
];

final installedApps = [
  for (final (index, app) in apps.take(6).indexed)
    InstalledPackage(
      uid: uint64(10000 + index),
      system: false,
      packageName: app.packageName,
      versionCode: uint64(app.versionCode - (index.isEven ? 1 : 0)),
      versionName: '1.2.$index',
      label: app.appName,
      launchable: true,
      vr: true,
      size: AppSize(
          app: app.size, data: uint64(124000000), cache: uint64(8000000)),
      isPackageRenamed: false,
    ),
];

final device = AdbDevice(
  name: 'Meta Quest 3',
  product: 'eureka',
  serial: '3A_DEMO_001',
  trueSerial: '3A_DEMO_001',
  transportId: '1',
  isWireless: false,
  batteryLevel: 76,
  isCharging: true,
  controllers: const HeadsetControllersInfo(
    left: ControllerInfo(batteryLevel: 85, status: ControllerStatusActive()),
    right: ControllerInfo(batteryLevel: 92, status: ControllerStatusActive()),
  ),
  spaceInfo:
      SpaceInfo(total: uint64(512000000000), available: uint64(186400000000)),
  installedPackages: installedApps,
  guardianPaused: false,
  proximityDisabled: false,
  storageConnected: false,
  usbSpeed: '5 Gbps',
);

final downloads = [
  for (final (index, app) in apps.take(5).indexed)
    DownloadEntry(
      path: '/Downloads/YAAS/${app.fullName}',
      name: app.fullName,
      timestamp: uint64(1788782400000 - index * 86400000),
      totalSize: app.size,
      packageName: app.packageName,
      versionCode: app.versionCode,
    ),
];

const details = AppDetailsResponse(
  packageName: 'org.example.orbit',
  appId: 'demo-orbit',
  displayName: 'Orbit Workshop',
  description: 'Build a little universe with your own hands.\n\n'
      'Assemble satellites, explore a peaceful space station, and send your '
      'creations into orbit. Every room is a new place to experiment.\n\n'
      'Play seated or standing, at your own pace. Invite a friend to your '
      'workshop and see what you can create together.',
  ratingAverage: 4.8,
  ratingCount: 128,
  notFound: false,
);

const reviews = AppReviewsResponse(
  appId: 'demo-orbit',
  total: 2,
  reviews: [
    AppReview(
      id: 'demo-review-1',
      authorDisplayName: 'Alex',
      authorAlias: 'stargazer',
      score: 5,
      reviewTitle: 'A lovely place to create',
      reviewDescription:
          'The workshop is easy to learn and full of small details. '
          'Building a satellite together was our favorite part.',
      date: '2026-09-01T12:00:00Z',
      reviewHelpfulCount: 24,
    ),
    AppReview(
      id: 'demo-review-2',
      authorDisplayName: 'Sam',
      score: 4,
      reviewTitle: 'Relaxing and imaginative',
      reviewDescription: 'A great way to unwind and try a new idea.',
      date: '2026-08-28T12:00:00Z',
      reviewHelpfulCount: 12,
    ),
  ],
);

class ScreenshotTasks extends TaskState {
  bool showTasks = false;

  void reveal() {
    showTasks = true;
    notifyListeners();
  }

  @override
  List<TaskInfo> get activeTasks => showTasks
      ? [
          _task(1, TaskKind.downloadInstall, apps[3].fullName, 0.255,
              'Pushing OBB 1/1 (51%)',
              currentStep: 2),
          _task(2, TaskKind.downloadInstall, apps[1].fullName, 0.083,
              'Downloading (8.3%) - 59.19 MB/s'),
          _task(3, TaskKind.download, apps[6].fullName, null,
              'Waiting to start download...',
              waiting: true),
        ]
      : [];

  TaskInfo _task(
      int id, TaskKind kind, String name, double? progress, String message,
      {bool waiting = false, int currentStep = 1}) {
    final totalSteps = kind == TaskKind.downloadInstall ? 2 : 1;
    return TaskInfo(
      taskId: id,
      kind: kind,
      taskName: name,
      status: waiting ? TaskStatus.waiting : TaskStatus.running,
      totalProgress: (currentStep - 1 + (progress ?? 0)) / totalSteps,
      currentStep: currentStep,
      totalSteps: totalSteps,
      stepProgress: progress,
      message: message,
      startTime: DateTime.utc(2026, 9, 7, 12),
    );
  }
}

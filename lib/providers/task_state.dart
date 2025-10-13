import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

class TaskInfo {
  final int taskId;
  final TaskKind kind;
  final String? taskName;
  final TaskStatus status;
  final double totalProgress;
  final int currentStep;
  final int totalSteps;
  final double? stepProgress;
  final String message;
  final DateTime startTime;
  DateTime? endTime;

  TaskInfo({
    required this.taskId,
    required this.kind,
    this.taskName,
    required this.status,
    required this.totalProgress,
    required this.currentStep,
    required this.totalSteps,
    required this.stepProgress,
    required this.message,
    required this.startTime,
    this.endTime,
  });

  TaskInfo copyWith({
    String? taskName,
    TaskStatus? status,
    double? totalProgress,
    int? currentStep,
    int? totalSteps,
    double? Function()? stepProgress,
    String? message,
    DateTime? endTime,
  }) {
    return TaskInfo(
      taskId: taskId,
      kind: kind,
      taskName: taskName ?? this.taskName,
      status: status ?? this.status,
      totalProgress: totalProgress ?? this.totalProgress,
      currentStep: currentStep ?? this.currentStep,
      totalSteps: totalSteps ?? this.totalSteps,
      stepProgress: stepProgress == null ? this.stepProgress : stepProgress(),
      message: message ?? this.message,
      startTime: startTime,
      endTime: endTime ?? this.endTime,
    );
  }

  bool get isFinished =>
      status == TaskStatus.completed ||
      status == TaskStatus.failed ||
      status == TaskStatus.cancelled;
}

class TaskState extends ChangeNotifier {
  final Map<int, TaskInfo> _tasks = {};
  final int _maxFinishedTasks = 50;

  List<TaskInfo> get activeTasks =>
      _tasks.values.where((task) => !task.isFinished).toList();

  List<TaskInfo> get recentTasks =>
      _tasks.values.where((task) => task.isFinished).toList()
        ..sort((a, b) => b.endTime!.compareTo(a.endTime!));

  TaskState() {
    TaskProgress.rustSignalStream.listen((event) {
      final progress = event.message;
      final taskId = progress.taskId.toInt();

      if (_tasks.containsKey(taskId)) {
        final oldTask = _tasks[taskId]!;
        _tasks[taskId] = oldTask.copyWith(
          taskName: progress.taskName,
          status: progress.status,
          totalProgress: progress.totalProgress,
          currentStep: progress.currentStep,
          totalSteps: progress.totalSteps,
          stepProgress: () => progress.stepProgress,
          message: progress.message,
          endTime: progress.status == TaskStatus.completed ||
                  progress.status == TaskStatus.failed ||
                  progress.status == TaskStatus.cancelled
              ? DateTime.now()
              : null,
        );

        if (oldTask.status != progress.status) {
          debugPrint(
              '[TaskState] Task $taskId (${progress.taskName ?? 'Unknown'}) '
              'status changed: ${oldTask.status} -> ${progress.status}');
        }

        // Log completion with duration
        if (progress.status == TaskStatus.completed ||
            progress.status == TaskStatus.failed ||
            progress.status == TaskStatus.cancelled) {
          final duration = DateTime.now().difference(oldTask.startTime);
          debugPrint(
              '[TaskState] Task $taskId completed in ${duration.inSeconds}s '
              'with status: ${progress.status}');
        }
      } else {
        _tasks[taskId] = TaskInfo(
          taskId: taskId,
          kind: progress.taskKind,
          taskName: progress.taskName,
          status: progress.status,
          totalProgress: progress.totalProgress,
          currentStep: progress.currentStep,
          totalSteps: progress.totalSteps,
          stepProgress: progress.stepProgress,
          message: progress.message,
          startTime: DateTime.now(),
        );

        debugPrint(
            '[TaskState] New task created: $taskId (${progress.taskName ?? 'Unknown'}), task kind: ${progress.taskKind}');
      }

      _cleanupOldTasks();
      notifyListeners();
    });
  }

  void _cleanupOldTasks() {
    final finishedTasks = _tasks.values
        .where((task) => task.isFinished)
        .toList()
      ..sort((a, b) => b.endTime!.compareTo(a.endTime!));

    if (finishedTasks.length > _maxFinishedTasks) {
      final tasksToRemove = finishedTasks.length - _maxFinishedTasks;
      debugPrint('[TaskState] Cleaning up $tasksToRemove old finished tasks '
          '(keeping $_maxFinishedTasks most recent)');

      for (var i = _maxFinishedTasks; i < finishedTasks.length; i++) {
        final task = finishedTasks[i];
        debugPrint('[TaskState] Removing old task: ${task.taskId} '
            '(${task.taskName ?? 'Unknown'})');
        _tasks.remove(task.taskId);
      }
    }
  }
}

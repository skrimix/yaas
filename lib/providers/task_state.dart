import 'package:flutter/material.dart';
import '../messages/all.dart';

class TaskInfo {
  final int taskId;
  final TaskType type;
  final TaskStatus status;
  final double stepProgress;
  final double totalProgress;
  final String message;
  final DateTime startTime;
  DateTime? endTime;

  TaskInfo({
    required this.taskId,
    required this.type,
    required this.status,
    required this.stepProgress,
    required this.totalProgress,
    required this.message,
    required this.startTime,
    this.endTime,
  });

  TaskInfo copyWith({
    TaskStatus? status,
    double? stepProgress,
    double? totalProgress,
    String? message,
    DateTime? endTime,
  }) {
    return TaskInfo(
      taskId: taskId,
      type: type,
      status: status ?? this.status,
      stepProgress: stepProgress ?? this.stepProgress,
      totalProgress: totalProgress ?? this.totalProgress,
      message: message ?? this.message,
      startTime: startTime,
      endTime: endTime ?? this.endTime,
    );
  }

  bool get isFinished =>
      status == TaskStatus.TASK_STATUS_COMPLETED ||
      status == TaskStatus.TASK_STATUS_FAILED ||
      status == TaskStatus.TASK_STATUS_CANCELLED;
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
        _tasks[taskId] = _tasks[taskId]!.copyWith(
          status: progress.status,
          stepProgress: progress.stepProgress,
          totalProgress: progress.totalProgress,
          message: progress.message,
          endTime: progress.status == TaskStatus.TASK_STATUS_COMPLETED ||
                  progress.status == TaskStatus.TASK_STATUS_FAILED ||
                  progress.status == TaskStatus.TASK_STATUS_CANCELLED
              ? DateTime.now()
              : null,
        );
      } else {
        _tasks[taskId] = TaskInfo(
          taskId: taskId,
          type: progress.type,
          status: progress.status,
          stepProgress: progress.stepProgress,
          totalProgress: progress.totalProgress,
          message: progress.message,
          startTime: DateTime.now(),
        );
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
      for (var i = _maxFinishedTasks; i < finishedTasks.length; i++) {
        _tasks.remove(finishedTasks[i].taskId);
      }
    }
  }
}

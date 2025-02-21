import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../messages/all.dart';
import '../providers/task_state.dart';

class TaskListDialog extends StatelessWidget {
  const TaskListDialog({super.key});

  String _getTaskTypeString(TaskType type) {
    switch (type) {
      case TaskType.TASK_TYPE_DOWNLOAD:
        return 'Download';
      case TaskType.TASK_TYPE_DOWNLOAD_INSTALL:
        return 'Download & Install';
      case TaskType.TASK_TYPE_INSTALL_APK:
        return 'Install APK';
      case TaskType.TASK_TYPE_INSTALL_LOCAL_APP:
        return 'Install Local App';
      case TaskType.TASK_TYPE_UNINSTALL:
        return 'Uninstall';
      default:
        return 'Unknown';
    }
  }

  String _getStatusString(TaskStatus status) {
    switch (status) {
      case TaskStatus.TASK_STATUS_WAITING:
        return 'Waiting';
      case TaskStatus.TASK_STATUS_RUNNING:
        return 'Running';
      case TaskStatus.TASK_STATUS_COMPLETED:
        return 'Completed';
      case TaskStatus.TASK_STATUS_FAILED:
        return 'Failed';
      case TaskStatus.TASK_STATUS_CANCELLED:
        return 'Cancelled';
      default:
        return 'Unknown';
    }
  }

  Color _getStatusColor(TaskStatus status) {
    switch (status) {
      case TaskStatus.TASK_STATUS_WAITING:
        return Colors.orange;
      case TaskStatus.TASK_STATUS_RUNNING:
        return Colors.blue;
      case TaskStatus.TASK_STATUS_COMPLETED:
        return Colors.green;
      case TaskStatus.TASK_STATUS_FAILED:
        return Colors.red;
      case TaskStatus.TASK_STATUS_CANCELLED:
        return Colors.grey;
      default:
        return Colors.grey;
    }
  }

  Widget _buildTaskItem(TaskInfo task) {
    return ListTile(
      title: Row(
        children: [
          Text(_getTaskTypeString(task.type)),
          const SizedBox(width: 8),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
            decoration: BoxDecoration(
              color: _getStatusColor(task.status).withValues(alpha: 0.1),
              border: Border.all(
                color: _getStatusColor(task.status).withValues(alpha: 0.5),
              ),
              borderRadius: BorderRadius.circular(4),
            ),
            child: Text(
              _getStatusString(task.status),
              style: TextStyle(
                color: _getStatusColor(task.status),
                fontSize: 12,
              ),
            ),
          ),
        ],
      ),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(task.message),
          if (!task.isFinished)
            LinearProgressIndicator(
              value: task.totalProgress,
              backgroundColor: Colors.grey.withValues(alpha: 0.1),
            ),
        ],
      ),
      trailing: Text(
        task.isFinished
            ? '${task.endTime!.hour}:${task.endTime!.minute.toString().padLeft(2, '0')}'
            : '${(task.totalProgress * 100).toInt()}%',
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 600, maxHeight: 400),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Text(
                    'Tasks',
                    style: TextStyle(
                      fontSize: 20,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  const Spacer(),
                  IconButton(
                    icon: const Icon(Icons.close),
                    onPressed: () => Navigator.of(context).pop(),
                  ),
                ],
              ),
              const Divider(),
              Expanded(
                child: DefaultTabController(
                  length: 2,
                  child: Column(
                    children: [
                      const TabBar(
                        tabs: [
                          Tab(text: 'Active'),
                          Tab(text: 'Recent'),
                        ],
                      ),
                      Expanded(
                        child: TabBarView(
                          children: [
                            _TaskList(
                              tasksSelector: (state) => state.activeTasks,
                              emptyMessage: 'No active tasks',
                              itemBuilder: _buildTaskItem,
                            ),
                            _TaskList(
                              tasksSelector: (state) => state.recentTasks,
                              emptyMessage: 'No recent tasks',
                              itemBuilder: _buildTaskItem,
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TaskList extends StatelessWidget {
  final List<TaskInfo> Function(TaskState) tasksSelector;
  final String emptyMessage;
  final Widget Function(TaskInfo) itemBuilder;

  const _TaskList({
    required this.tasksSelector,
    required this.emptyMessage,
    required this.itemBuilder,
  });

  @override
  Widget build(BuildContext context) {
    return Consumer<TaskState>(
      builder: (context, taskState, child) {
        final tasks = tasksSelector(taskState);
        if (tasks.isEmpty) {
          return Center(
            child: Text(
              emptyMessage,
              style: TextStyle(
                color: Theme.of(context)
                    .colorScheme
                    .onSurface
                    .withValues(alpha: 0.5),
              ),
            ),
          );
        }
        return ListView.builder(
          itemCount: tasks.length,
          itemBuilder: (context, index) => itemBuilder(tasks[index]),
        );
      },
    );
  }
}

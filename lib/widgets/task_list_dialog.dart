import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../src/bindings/bindings.dart';
import '../providers/task_state.dart';

class TaskListDialog extends StatefulWidget {
  final int initialTabIndex;

  const TaskListDialog({
    super.key,
    this.initialTabIndex = 0,
  });

  @override
  State<TaskListDialog> createState() => _TaskListDialogState();
}

class _TaskListDialogState extends State<TaskListDialog>
    with SingleTickerProviderStateMixin {
  late final TabController _tabController;
  TaskState? _previousTaskState;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(
      length: 2,
      initialIndex: widget.initialTabIndex,
      vsync: this,
    );
  }

  @override
  void dispose() {
    _tabController.dispose();
    super.dispose();
  }

  void _handleTaskStateChange(TaskState taskState) {
    if (_previousTaskState != null &&
        _previousTaskState!.activeTasks.isEmpty &&
        taskState.activeTasks.isEmpty) {
      _tabController.animateTo(1); // Switch to recent tab
    }
    _previousTaskState = taskState;
  }

  String _getTaskTypeString(TaskType type) {
    switch (type) {
      case TaskType.download:
        return 'Download';
      case TaskType.downloadInstall:
        return 'Download & Install';
      case TaskType.installApk:
        return 'Install APK';
      case TaskType.installLocalApp:
        return 'Install Local App';
      case TaskType.uninstall:
        return 'Uninstall';
    }
  }

  String _getStatusString(TaskStatus status) {
    switch (status) {
      case TaskStatus.waiting:
        return 'Waiting';
      case TaskStatus.running:
        return 'Running';
      case TaskStatus.completed:
        return 'Completed';
      case TaskStatus.failed:
        return 'Failed';
      case TaskStatus.cancelled:
        return 'Cancelled';
    }
  }

  Color _getStatusColor(TaskStatus status) {
    switch (status) {
      case TaskStatus.waiting:
        return Colors.orange;
      case TaskStatus.running:
        return Colors.blue;
      case TaskStatus.completed:
        return Colors.green;
      case TaskStatus.failed:
        return Colors.red;
      case TaskStatus.cancelled:
        return Colors.grey;
    }
  }

  Widget _buildTab(BuildContext context, String label, int count) {
    return Tab(
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(label),
          if (count > 0) ...[
            const SizedBox(width: 8),
            Container(
              padding: const EdgeInsets.symmetric(
                horizontal: 6,
                vertical: 2,
              ),
              decoration: BoxDecoration(
                color: Theme.of(context)
                    .colorScheme
                    .primary
                    .withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Text(
                count.toString(),
                style: TextStyle(
                  fontSize: 12,
                  color: Theme.of(context).colorScheme.primary,
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildTaskItem(BuildContext context, TaskInfo task) {
    final taskName = task.taskName ?? "Unknown";

    return ListTile(
      title: Row(
        children: [
          Expanded(
            child: Align(
              alignment: Alignment.centerLeft,
              child: Tooltip(
                message: taskName,
                waitDuration: const Duration(milliseconds: 500),
                child: Text(
                  taskName,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
          ),
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
          Row(
            children: [
              Text(
                _getTaskTypeString(task.type),
                style: TextStyle(
                  fontSize: 12,
                  color: Theme.of(context)
                      .colorScheme
                      .onSurface
                      .withValues(alpha: 0.7),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Tooltip(
                    message: task.message,
                    waitDuration: const Duration(milliseconds: 500),
                    child: Text(
                      task.message, // TODO: make copyable
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ),
              ),
            ],
          ),
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
              Expanded(
                child: Consumer<TaskState>(
                  builder: (context, taskState, child) {
                    _handleTaskStateChange(taskState);
                    final activeCount = taskState.activeTasks.length;
                    final recentCount = taskState.recentTasks.length;
                    return Column(
                      children: [
                        TabBar(
                          controller: _tabController,
                          tabs: [
                            _buildTab(context, 'Active', activeCount),
                            _buildTab(context, 'Recent', recentCount),
                          ],
                        ),
                        Expanded(
                          child: TabBarView(
                            controller: _tabController,
                            children: [
                              _TaskList(
                                tasks: taskState.activeTasks,
                                emptyMessage: 'No active tasks',
                                itemBuilder: _buildTaskItem,
                              ),
                              _TaskList(
                                tasks: taskState.recentTasks,
                                emptyMessage: 'No recent tasks',
                                itemBuilder: _buildTaskItem,
                              ),
                            ],
                          ),
                        ),
                      ],
                    );
                  },
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
  final List<TaskInfo> tasks;
  final String emptyMessage;
  final Widget Function(BuildContext, TaskInfo) itemBuilder;

  const _TaskList({
    required this.tasks,
    required this.emptyMessage,
    required this.itemBuilder,
  });

  @override
  Widget build(BuildContext context) {
    if (tasks.isEmpty) {
      return Center(
        child: Text(
          emptyMessage,
          style: TextStyle(
            color:
                Theme.of(context).colorScheme.onSurface.withValues(alpha: 0.5),
          ),
        ),
      );
    }
    return ListView.builder(
      itemCount: tasks.length,
      itemBuilder: (context, index) => itemBuilder(context, tasks[index]),
    );
  }
}

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../../src/bindings/bindings.dart';
import '../../providers/task_state.dart';
import '../../src/l10n/app_localizations.dart';
import '../../utils/utils.dart';

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
  final Set<int> _cancellingTaskIds = <int>{};

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
    _cancellingTaskIds
        .removeWhere((id) => !taskState.activeTasks.any((t) => t.taskId == id));
    _previousTaskState = taskState;
  }

  String _getTaskKindString(TaskKind kind) {
    final l10n = AppLocalizations.of(context);
    switch (kind) {
      case TaskKind.download:
        return l10n.taskKindDownload;
      case TaskKind.downloadInstall:
        return l10n.taskKindDownloadInstall;
      case TaskKind.installApk:
        return l10n.taskKindInstallApk;
      case TaskKind.installLocalApp:
        return l10n.taskKindInstallLocalApp;
      case TaskKind.uninstall:
        return l10n.taskKindUninstall;
      case TaskKind.backupApp:
        return l10n.taskKindBackupApp;
      case TaskKind.restoreBackup:
        return l10n.taskKindRestoreBackup;
      case TaskKind.donateApp:
        return l10n.taskKindDonateApp;
    }
  }

  String _getStatusString(TaskStatus status) {
    final l10n = AppLocalizations.of(context);
    switch (status) {
      case TaskStatus.waiting:
        return l10n.taskStatusWaiting;
      case TaskStatus.running:
        return l10n.taskStatusRunning;
      case TaskStatus.completed:
        return l10n.taskStatusCompleted;
      case TaskStatus.failed:
        return l10n.taskStatusFailed;
      case TaskStatus.cancelled:
        return l10n.taskStatusCancelled;
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
    final l10n = AppLocalizations.of(context);
    final taskName = task.taskName ?? l10n.taskUnknown;

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
          if (task.isFinished) ...[
            Text(
              '${task.endTime!.hour}:${task.endTime!.minute.toString().padLeft(2, '0')}',
              style: TextStyle(
                fontSize: 12,
                color: Theme.of(context)
                    .colorScheme
                    .onSurface
                    .withValues(alpha: 0.7),
              ),
            ),
            const SizedBox(width: 8),
          ],
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
                _getTaskKindString(task.kind),
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
                    child: InkWell(
                      onTap: () => copyToClipboard(
                        context,
                        task.message,
                        description: task.message,
                      ),
                      child: Text(
                        task.message,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ),
                ),
              ),
              if (!task.isFinished) ...[
                const SizedBox(width: 8),
                Text(
                  '${task.currentStep}/${task.totalSteps}',
                  style: TextStyle(
                    fontSize: 12,
                    color: Theme.of(context)
                        .colorScheme
                        .onSurface
                        .withValues(alpha: 0.6),
                  ),
                ),
              ],
            ],
          ),
          if (!task.isFinished)
            LinearProgressIndicator(
              value: task.stepProgress, // null -> indeterminate
              backgroundColor: Colors.grey.withValues(alpha: 0.1),
            ),
        ],
      ),
      trailing: task.isFinished
          ? null
          : Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (_cancellingTaskIds.contains(task.taskId))
                  const SizedBox(
                    width: 36,
                    height: 36,
                    child: Padding(
                      padding: EdgeInsets.all(8),
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                  )
                else
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    icon: const Icon(Icons.close),
                    tooltip: l10n.cancelTask,
                    onPressed: () {
                      setState(() {
                        _cancellingTaskIds.add(task.taskId);
                      });
                      TaskCancelRequest(
                        taskId: Uint64.fromBigInt(BigInt.from(task.taskId)),
                      ).sendSignalToRust();
                    },
                  ),
              ],
            ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
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
                  Text(
                    l10n.tasksTitle,
                    style: TextStyle(
                      fontSize: 20,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  const Spacer(),
                  Consumer<TaskState>(
                    builder: (context, taskState, _) {
                      if (taskState.recentTasks.isEmpty) {
                        return const SizedBox.shrink();
                      }
                      return IconButton(
                        icon: const Icon(Icons.delete_sweep_outlined),
                        tooltip: l10n.clearRecentTasks,
                        onPressed: () {
                          taskState.clearRecentTasks();
                        },
                      );
                    },
                  ),
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
                            _buildTab(
                                context, l10n.tasksTabActive, activeCount),
                            _buildTab(
                                context, l10n.tasksTabRecent, recentCount),
                          ],
                        ),
                        Expanded(
                          child: TabBarView(
                            controller: _tabController,
                            children: [
                              _TaskList(
                                tasks: taskState.activeTasks,
                                emptyMessage: l10n.tasksEmptyActive,
                                itemBuilder: _buildTaskItem,
                              ),
                              _TaskList(
                                tasks: taskState.recentTasks,
                                emptyMessage: l10n.tasksEmptyRecent,
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

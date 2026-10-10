import 'package:flutter/material.dart';

/// Spacing shared by top-level pages.
class PageLayout {
  static const double horizontalPadding = 16;
  static const double headerHeight = 48;
}

/// Title row shown at the top of a page, with trailing actions and an
/// optional secondary row below.
///
/// Actions are laid out in a row that fills the space after the title, so an
/// [Expanded] action (such as a search field) takes the remaining width.
class PageHeader extends StatelessWidget {
  const PageHeader({
    super.key,
    required this.title,
    this.actions = const [],
    this.bottom,
  });

  final String title;
  final List<Widget> actions;
  final Widget? bottom;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(
        PageLayout.horizontalPadding,
        16,
        PageLayout.horizontalPadding,
        12,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(
            height: PageLayout.headerHeight,
            child: Row(
              children: [
                Text(
                  title,
                  style: Theme.of(context).textTheme.headlineSmall,
                  maxLines: 1,
                ),
                const SizedBox(width: 16),
                Expanded(
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: actions,
                  ),
                ),
              ],
            ),
          ),
          if (bottom != null) ...[
            const SizedBox(height: 8),
            bottom!,
          ],
        ],
      ),
    );
  }
}

/// Standard top-level page: a [PageHeader] above the page body.
///
/// With [maxWidth], the header is centered and aligned with a body whose
/// content is constrained to the same width.
class PageScaffold extends StatelessWidget {
  const PageScaffold({
    super.key,
    required this.title,
    required this.body,
    this.actions = const [],
    this.headerBottom,
    this.bottomBar,
    this.maxWidth,
  });

  final String title;
  final Widget body;
  final List<Widget> actions;
  final Widget? headerBottom;
  final Widget? bottomBar;
  final double? maxWidth;

  @override
  Widget build(BuildContext context) {
    Widget header = PageHeader(
      title: title,
      actions: actions,
      bottom: headerBottom,
    );
    final maxWidth = this.maxWidth;
    if (maxWidth != null) {
      header = Center(
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxWidth: maxWidth + PageLayout.horizontalPadding * 2,
          ),
          child: header,
        ),
      );
    }

    return Scaffold(
      bottomNavigationBar: bottomBar,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            header,
            Expanded(child: body),
          ],
        ),
      ),
    );
  }
}

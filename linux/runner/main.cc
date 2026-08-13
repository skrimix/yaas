#include "my_application.h"

int main(int argc, char** argv) {
  // disable mangohud, it may crash our video player
  g_setenv("DISABLE_MANGOHUD", "1", TRUE);

  g_autoptr(MyApplication) app = my_application_new();
  return g_application_run(G_APPLICATION(app), argc, argv);
}

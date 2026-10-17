/* The pthread clock functions libstdc++ 10 was configured to call (glibc 2.30+),
 * declared for the 2.28 sysroot. Declarations only: a call that reaches the link
 * fails build.sh's glibc floor check. */
#ifdef __cplusplus
#include <pthread.h>
#include <time.h>

extern "C" {
int pthread_mutex_clocklock(pthread_mutex_t *, clockid_t, const struct timespec *);
int pthread_cond_clockwait(pthread_cond_t *, pthread_mutex_t *, clockid_t,
                           const struct timespec *);
int pthread_rwlock_clockrdlock(pthread_rwlock_t *, clockid_t, const struct timespec *);
int pthread_rwlock_clockwrlock(pthread_rwlock_t *, clockid_t, const struct timespec *);
}
#endif

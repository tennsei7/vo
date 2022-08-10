/*
 * The Shadow Simulator
 * See LICENSE for licensing information
 */

#include <glib.h>
#include <pthread.h>
#include <string.h>

#include "lib/logger/logger.h"
#include "main/core/scheduler/scheduler_policy.h"
#include "main/core/support/definitions.h"
#include "main/host/host.h"
#include "main/utility/priority_queue.h"
#include "main/utility/utility.h"

typedef struct _HostSingleThreadData HostSingleThreadData;
struct _HostSingleThreadData {
    /* used to cache getHosts() result for memory management as needed */
    GQueue* allHosts;
    /* all hosts that have been assigned to this worker for event processing but not yet processed this round */
    GQueue* unprocessedHosts;
    /* during each round, hosts whose events have been processed are moved from unprocessedHosts to here */
    GQueue* processedHosts;
    SimulationTime currentBarrier;
};

struct _SchedulerPolicy {
    GHashTable* hostToQueueDataMap;
    GHashTable* threadToThreadDataMap;
    GHashTable* hostToThreadMap;
    MAGIC_DECLARE;
};

typedef struct _HostSingleSearchState HostSingleSearchState;
struct _HostSingleSearchState {
    SchedulerPolicy* data;
    SimulationTime nextEventTime;
};

static HostSingleThreadData* _threaddata_new() {
    HostSingleThreadData* tdata = g_new0(HostSingleThreadData, 1);

    tdata->unprocessedHosts = g_queue_new();
    tdata->processedHosts = g_queue_new();

    return tdata;
}

static void _threaddata_free(HostSingleThreadData* tdata) {
    if(tdata) {
        if(tdata->allHosts) {
            g_queue_free(tdata->allHosts);
        }
        if(tdata->unprocessedHosts) {
            g_queue_free(tdata->unprocessedHosts);
        }
        if(tdata->processedHosts) {
            g_queue_free(tdata->processedHosts);
        }
        g_free(tdata);
    }
}

/* this must be run synchronously, or the call must be protected by locks */
void schedulerpolicy_addHost(SchedulerPolicy* policy, Host* host, pthread_t randomThread) {
    MAGIC_ASSERT(policy);

    /* each host has its own queue */
    if (!g_hash_table_lookup(policy->hostToQueueDataMap, host)) {
        g_hash_table_replace(policy->hostToQueueDataMap, host, eventqueue_new());
    }

    /* each thread keeps track of the hosts it needs to run */
    pthread_t assignedThread = (randomThread != 0) ? randomThread : pthread_self();
    HostSingleThreadData* tdata =
        g_hash_table_lookup(policy->threadToThreadDataMap, GUINT_TO_POINTER(assignedThread));
    if(!tdata) {
        tdata = _threaddata_new();
        g_hash_table_replace(
            policy->threadToThreadDataMap, GUINT_TO_POINTER(assignedThread), tdata);
    }
    g_queue_push_tail(tdata->unprocessedHosts, host);

    /* finally, store the host-to-thread mapping */
    g_hash_table_replace(policy->hostToThreadMap, host, GUINT_TO_POINTER(assignedThread));
}

static void concat_queue_iter(Host* hostItem, GQueue* userQueue) {
    g_queue_push_tail(userQueue, hostItem);
}

GQueue* schedulerpolicy_getAssignedHosts(SchedulerPolicy* policy) {
    MAGIC_ASSERT(policy);
    HostSingleThreadData* tdata =
        g_hash_table_lookup(policy->threadToThreadDataMap, GUINT_TO_POINTER(pthread_self()));
    if(!tdata) {
        return NULL;
    }
    if(g_queue_is_empty(tdata->unprocessedHosts)) {
        return tdata->processedHosts;
    }
    if(g_queue_is_empty(tdata->processedHosts)) {
        return tdata->unprocessedHosts;
    }
    if(tdata->allHosts) {
        g_queue_free(tdata->allHosts);
    }
    tdata->allHosts = g_queue_copy(tdata->processedHosts);
    g_queue_foreach(tdata->unprocessedHosts, (GFunc)concat_queue_iter, tdata->allHosts);
    return tdata->allHosts;
}

SimulationTime schedulerpolicy_push(SchedulerPolicy* policy, Event* event, Host* srcHost,
                                    Host* dstHost, SimulationTime barrier) {
    MAGIC_ASSERT(policy);

    /* non-local events must be properly delayed so the event wont show up at another host
     * before the next scheduling interval. if the thread scheduler guaranteed to always run
     * the minimum time event accross all of its assigned hosts, then we would only need to
     * do the time adjustment if the srcThread and dstThread are not identical. however,
     * the logic of this policy allows a thread to run all events from a given host before
     * moving on to the next host, so we must adjust the time whenever the srcHost and
     * dstHost are not the same. */
    SimulationTime eventTime = event_getTime(event);

    if(srcHost != dstHost && eventTime < barrier) {
        event_setTime(event, barrier);
        debug("Inter-host event time %" G_GUINT64_FORMAT " changed to %" G_GUINT64_FORMAT " "
              "to ensure event causality",
              eventTime, barrier);
    }

    /* we want to track how long this thread spends idle waiting to push the event */
    HostSingleThreadData* tdata =
        g_hash_table_lookup(policy->threadToThreadDataMap, GUINT_TO_POINTER(pthread_self()));

    /* get the queue for the destination */
    ThreadSafeEventQueue* qdata = g_hash_table_lookup(policy->hostToQueueDataMap, dstHost);
    utility_debugAssert(qdata);

    eventTime = event_getTime(event);

    /* 'deliver' the event to the destination queue */
    eventqueue_push(qdata, event);

    return eventTime;
}

Event* schedulerpolicy_pop(SchedulerPolicy* policy, SimulationTime barrier) {
    MAGIC_ASSERT(policy);

    /* figure out which hosts we should be checking */
    HostSingleThreadData* tdata =
        g_hash_table_lookup(policy->threadToThreadDataMap, GUINT_TO_POINTER(pthread_self()));
    /* if there is no tdata, that means this thread didn't get any hosts assigned to it */
    if(!tdata) {
        /* this thread will remain idle */
        return NULL;
    }

    if(barrier > tdata->currentBarrier) {
        tdata->currentBarrier = barrier;

        /* make sure all of the hosts that were processed last time get processed in the next round */
        if(g_queue_is_empty(tdata->unprocessedHosts) && !g_queue_is_empty(tdata->processedHosts)) {
            GQueue* swap = tdata->unprocessedHosts;
            tdata->unprocessedHosts = tdata->processedHosts;
            tdata->processedHosts = swap;
        } else {
            while(!g_queue_is_empty(tdata->processedHosts)) {
                g_queue_push_tail(tdata->unprocessedHosts, g_queue_pop_head(tdata->processedHosts));
            }
        }
    }

    while(!g_queue_is_empty(tdata->unprocessedHosts)) {
        Host* host = g_queue_peek_head(tdata->unprocessedHosts);
        ThreadSafeEventQueue* qdata = g_hash_table_lookup(policy->hostToQueueDataMap, host);
        utility_debugAssert(qdata);

        Event* nextEvent = NULL;
        SimulationTime eventTime = eventqueue_nextEventTime(qdata);

        if(eventTime != SIMTIME_INVALID && eventTime < barrier) {
            nextEvent = eventqueue_pop(qdata);
        }

        if(nextEvent != NULL) {
            return nextEvent;
        }
        /* this host is done, store it in the processed queue and then
         * try the next host if we still have more */
        g_queue_push_tail(tdata->processedHosts, g_queue_pop_head(tdata->unprocessedHosts));
    }

    /* if we make it here, all hosts for this thread have no more events before barrier */
    return NULL;
}

EmulatedTime schedulerpolicy_nextHostEventTime(SchedulerPolicy* policy, Host* host) {
    MAGIC_ASSERT(policy);

    /* figure out which hosts we should be checking */
    HostSingleThreadData* tdata =
        g_hash_table_lookup(policy->threadToThreadDataMap, GUINT_TO_POINTER(pthread_self()));
    utility_debugAssert(tdata);

    ThreadSafeEventQueue* qdata = g_hash_table_lookup(policy->hostToQueueDataMap, host);
    utility_debugAssert(qdata);

    SimulationTime nextEventSimTime = eventqueue_nextEventTime(qdata);
    EmulatedTime nextEventEmuTime = EMUTIME_INVALID;
    if (nextEventSimTime != SIMTIME_INVALID) {
        nextEventEmuTime = emutime_add_simtime(EMUTIME_SIMULATION_START, nextEventSimTime);
        utility_debugAssert(nextEventEmuTime != EMUTIME_INVALID);
    }

    return nextEventEmuTime;
}

static void _schedulerpolicy_findMinTime(Host* host, HostSingleSearchState* state) {
    ThreadSafeEventQueue* qdata = g_hash_table_lookup(state->data->hostToQueueDataMap, host);
    utility_debugAssert(qdata);

    SimulationTime nextEventTime = eventqueue_nextEventTime(qdata);
    if (nextEventTime != SIMTIME_INVALID) {
        state->nextEventTime = MIN(state->nextEventTime, nextEventTime);
    }
}

SimulationTime schedulerpolicy_getNextTime(SchedulerPolicy* policy) {
    MAGIC_ASSERT(policy);

    /* set up state that we need for the foreach queue iterator */
    HostSingleSearchState searchState;
    memset(&searchState, 0, sizeof(HostSingleSearchState));
    searchState.data = policy;
    searchState.nextEventTime = SIMTIME_MAX;

    HostSingleThreadData* tdata =
        g_hash_table_lookup(policy->threadToThreadDataMap, GUINT_TO_POINTER(pthread_self()));
    if(tdata) {
        /* make sure we get all hosts, which are probably held in the processedHosts queue between rounds */
        g_queue_foreach(tdata->unprocessedHosts, (GFunc)_schedulerpolicy_findMinTime, &searchState);
        g_queue_foreach(tdata->processedHosts, (GFunc)_schedulerpolicy_findMinTime, &searchState);
    }
    debug("next event at time %" G_GUINT64_FORMAT, searchState.nextEventTime);

    return searchState.nextEventTime;
}

void schedulerpolicy_free(SchedulerPolicy* policy) {
    MAGIC_ASSERT(policy);

    g_hash_table_destroy(policy->hostToQueueDataMap);
    g_hash_table_destroy(policy->threadToThreadDataMap);
    g_hash_table_destroy(policy->hostToThreadMap);

    MAGIC_CLEAR(policy);
    g_free(policy);
}

SchedulerPolicy* schedulerpolicy_new() {
    SchedulerPolicy* policy = g_new0(SchedulerPolicy, 1);
    MAGIC_INIT(policy);

    policy->hostToQueueDataMap =
        g_hash_table_new_full(g_direct_hash, g_direct_equal, NULL, (GDestroyNotify)eventqueue_free);
    policy->threadToThreadDataMap = g_hash_table_new_full(
        g_direct_hash, g_direct_equal, NULL, (GDestroyNotify)_threaddata_free);
    policy->hostToThreadMap = g_hash_table_new(g_direct_hash, g_direct_equal);

    return policy;
}
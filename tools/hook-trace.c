/*
 * Allocator hook-trace over the public cJSON API.
 * Build twice: against cJSON.c and against libcjson_ffi.a; diff the outputs.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../cJSON.h"

typedef struct {
    size_t size;
    unsigned char *block;
} alloc_rec;

#define MAX_ALLOCS 4096
static alloc_rec recs[MAX_ALLOCS];
static size_t nrecs = 0;
static int fail_after = -1; /* fail the Nth allocation (0-based); -1 = never */
static int alloc_count = 0;

static void *trace_malloc(size_t sz)
{
    void *p;
    if (fail_after >= 0 && alloc_count == fail_after)
    {
        alloc_count++;
        return NULL;
    }
    alloc_count++;
    p = malloc(sz);
    if (p != NULL && nrecs < MAX_ALLOCS)
    {
        recs[nrecs].size = sz;
        recs[nrecs].block = (unsigned char *)p;
        nrecs++;
    }
    printf("M %zu\n", sz);
    return p;
}

static void trace_free(void *p)
{
    size_t i;
    size_t sz = 0;
    if (p == NULL)
    {
        return;
    }
    for (i = 0; i < nrecs; i++)
    {
        if (recs[i].block == (unsigned char *)p)
        {
            sz = recs[i].size;
            recs[i].block = NULL;
            break;
        }
    }
    printf("F %zu\n", sz);
    free(p);
}

static void reset_trace(void)
{
    size_t i;
    for (i = 0; i < nrecs; i++)
    {
        if (recs[i].block != NULL)
        {
            free(recs[i].block);
        }
    }
    nrecs = 0;
    alloc_count = 0;
}

static void run_case(const char *label, int fail)
{
    cJSON_Hooks hooks;
    cJSON *root;
    char *printed;
    const char *input = "{\"a\":1,\"b\":[true,null,\"x\"]}";

    fail_after = fail;
    reset_trace();
    hooks.malloc_fn = trace_malloc;
    hooks.free_fn = trace_free;
    cJSON_InitHooks(&hooks);

    printf("BEGIN %s fail_after=%d\n", label, fail);
    root = cJSON_Parse(input);
    if (root == NULL)
    {
        printf("parse: NULL\n");
    }
    else
    {
        printf("parse: ok\n");
        printed = cJSON_PrintUnformatted(root);
        if (printed == NULL)
        {
            printf("print: NULL\n");
        }
        else
        {
            printf("print: %s\n", printed);
            cJSON_free(printed);
        }
        printed = cJSON_Print(root);
        if (printed != NULL)
        {
            printf("pretty_len: %zu\n", strlen(printed));
            cJSON_free(printed);
        }
        {
            cJSON *dup = cJSON_Duplicate(root, 1);
            if (dup != NULL)
            {
                printf("dup: ok\n");
                cJSON_Delete(dup);
            }
            else
            {
                printf("dup: NULL\n");
            }
        }
        cJSON_Delete(root);
    }
    printf("END %s\n", label);
    cJSON_InitHooks(NULL);
    reset_trace();
}

int main(void)
{
    int i;
    run_case("happy", -1);
    for (i = 0; i < 8; i++)
    {
        char label[32];
        snprintf(label, sizeof(label), "fail_%d", i);
        run_case(label, i);
    }
    return 0;
}

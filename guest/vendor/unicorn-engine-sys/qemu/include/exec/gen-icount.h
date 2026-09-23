#ifndef GEN_ICOUNT_H
#define GEN_ICOUNT_H

#include "qemu/timer.h"

/* Helpers for instruction counting code generation.  */

static inline void gen_io_start(TCGContext *tcg_ctx)
{
    TCGv_i32 tmp = tcg_const_i32(tcg_ctx, 1);
    tcg_gen_st_i32(tcg_ctx, tmp, tcg_ctx->cpu_env,
                   offsetof(ArchCPU, parent_obj.can_do_io) -
                   offsetof(ArchCPU, env));
    tcg_temp_free_i32(tcg_ctx, tmp);
}

/*
 * cpu->can_do_io is cleared automatically at the beginning of
 * each translation block.  The cost is minimal and only paid
 * for -icount, plus it would be very easy to forget doing it
 * in the translator.  Therefore, backends only need to call
 * gen_io_start.
 */
static inline void gen_io_end(TCGContext *tcg_ctx)
{
    TCGv_i32 tmp = tcg_const_i32(tcg_ctx, 0);
    tcg_gen_st_i32(tcg_ctx, tmp, tcg_ctx->cpu_env,
                   offsetof(ArchCPU, parent_obj.can_do_io) -
                   offsetof(ArchCPU, env));
    tcg_temp_free_i32(tcg_ctx, tmp);
}

static inline void gen_tb_start(TCGContext *tcg_ctx, TranslationBlock *tb)
{
    TCGLabel *continue_execution = gen_new_label(tcg_ctx);
    TCGv_i32 exit_request = tcg_temp_new_i32(tcg_ctx);

    /*
     * Keep helper arguments inside the slow-path basic block.  In particular,
     * no temporary may be live across this branch: the old TCG liveness pass
     * treats conditional branches as basic-block boundaries.
     */
    tcg_gen_ld_i32(tcg_ctx, exit_request, tcg_ctx->cpu_env,
                   offsetof(ArchCPU, neg.icount_decr) -
                   offsetof(ArchCPU, env));
    tcg_gen_brcondi_i32(tcg_ctx, TCG_COND_GE, exit_request, 0,
                        continue_execution);
    tcg_temp_free_i32(tcg_ctx, exit_request);

    TCGv_ptr puc = tcg_const_ptr(tcg_ctx, tcg_ctx->uc);
    TCGv_i32 tmp = tcg_const_i32(tcg_ctx, 0);
    if (tcg_ctx->delay_slot_flag != NULL) {
        tcg_gen_mov_i32(tcg_ctx, tmp, tcg_ctx->delay_slot_flag);
    }
    gen_helper_check_exit_request(tcg_ctx, puc, tmp);
    tcg_temp_free_i32(tcg_ctx, tmp);
    tcg_temp_free_ptr(tcg_ctx, puc);
    gen_set_label(tcg_ctx, continue_execution);
}

static inline void gen_tb_end(TCGContext *tcg_ctx, TranslationBlock *tb, int num_insns)
{
    if (tcg_ctx->delay_slot_flag != NULL){
        tcg_temp_free_i32(tcg_ctx, tcg_ctx->delay_slot_flag);
    }
    tcg_ctx->delay_slot_flag = NULL;
    if (tb_cflags(tb) & CF_USE_ICOUNT) {
        /* Update the num_insn immediate parameter now that we know
         * the actual insn count.  */
        tcg_set_insn_param(tcg_ctx->icount_start_insn, 1, num_insns);
    }

    tcg_gen_exit_tb(tcg_ctx, tb, TB_EXIT_REQUESTED);
}

#endif

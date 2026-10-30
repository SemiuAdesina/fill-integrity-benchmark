// Verifier-owned out-of-process driver. This binary links no candidate code.

#include <array>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <signal.h>
#include <string>
#include <sys/types.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>
#include <vector>

namespace {

struct Case { const char* name; const char* digest; int expected_rc; bool control; };

constexpr Case kCases[] = {
{"a_delivered_value_and_outcome","a90e00a5461c0e30a98848e9737951a9bcafeb781e94675bde6ae6f5f27446eb",0,false},
{"a_job_ids_are_unique_and_increasing","a15d3a94950aaa53c938dddf0f0c9991cd495e151ba7a769e59da2ba32ce0a15",0,false},
{"a_journal_orders_start_park_accept_settle_release","4d57080a2dc65996088eaf92dcb11debb6e0c3ff86ea35cd7f782d35891493ae",0,false},
{"a_launch_parks_and_reports_unsettled","82d5d19256d9274540187f504497d2fd8153dda7589e8d45395a468191196edd",0,false},
{"a_no_progress_without_stepping","1310bde56c93ed7177b67016a5397d3df2f42b861730ea975f6415d26520f2f7",0,false},
{"a_settle_releases_storage_immediately","1b8b49559038d68777caccb13b629fb4a656b1ab633eaa17159e360f1eb92dc6",0,false},
{"a_step_delivers_one_ticket_only","09036154fdda89c52b9152cb03817074acd0d31facf46531479fc9a641d850a9",0,false},
{"b_cancel_after_settle_is_harmless","1178643c40e5641c2572f01cd03b3b4b27f26996859e61405d8deb1c8a675bb6",0,false},
{"b_cancel_releases_storage_and_runs_cleanup","a6cccaf9e4ed8e6fdf5afc8c45657042de65a772c438da884de673dd32f599c7",0,false},
{"b_cancel_then_ticket_is_rejected","a59a452230154ec74913ba2c72893a487fe6ade1bfecf61d4e8f398b048733c1",0,false},
{"b_cancel_unknown_job_is_harmless","7a86930da5969c3fd73fbcf61b8feb82ef9e2c1bb2d38abdc8f7549d04b6009a",0,false},
{"b_cancel_while_parked_settles_cancelled","28175c502849cc17c0382888918b8d6d4486b41c6cbf0e640377d5d431c252b3",0,false},
{"b_cancelled_job_never_resumes","2d23f5d845f17dad1c2910e091a7e964fe25724a064aa2d6765573e8ab0c6055",0,false},
{"b_complete_then_cancel_then_complete","bb4c7cab0735ec621154ecd31e4eb1680bf1475dc0623a2a2848b7fa9a838f2e",0,false},
{"b_repeated_cancel_settles_once","dfb9045e8a007b500a78c1276ec1437c37602fc36b80c1464efc6258564b3cb5",0,false},
{"c_many_stale_tickets_all_rejected","40714cee2c31939971675222992cef3544020b179e4f701dceffb0de3947e6a0",0,false},
{"c_released_slot_is_handed_out_again","42d481b0f80470dc3a2a9c6c71f9b0abda6d065db79a93ec7dbad374a9343fbd",0,false},
{"c_slot_epoch_advances_per_reuse","e5fbd3f5be11dac8d7972f2cd2bcc2ed198840610ed80ac6ca748d2d92976e79",0,false},
{"c_stale_fault_ticket_does_not_fault_new_occupant","2ac588be846079a7528a078c240723fe927608cc32c7d36b33f79dfbf21ddd87",0,false},
{"c_stale_ticket_does_not_destroy_new_occupant","522ce4ca76eb2e98cd8b8f6157d185d28b7a2d03d75ef373588f6d07abb93559",0,false},
{"c_stale_ticket_does_not_resume_new_occupant","0b9dea594e0306a0d8730c5e2364e26e292f5858f16ec396daec9d3a59c5215f",0,false},
{"d_cancel_with_child_destroys_inner_first","803626ea1ee40a7fcc698a807a9ab188ae8ccf4bd0ba604c958aa14d5489999a",0,false},
{"d_child_destroyed_before_parent_continues","06e5817b7dd9797ae84b0617127385d4a07784d9bb68476c57fd8b6eaea8299c",0,false},
{"d_child_slot_returns_to_arena","5bfe5c205b3873d776297f9e79586f5019ff458d3fe2e7d0db9c9961fd5d097e",0,false},
{"d_child_value_reaches_parent","dbea5634231df90f9b2ac72614d78d874e4418460f053cea72f95d3d56223a4e",0,false},
{"d_nested_two_levels","0a34b2731b078e428ae14f4945675ac96671226c852e6912f039de0320ece466",0,false},
{"d_parent_continued_reports_child_gone","b360a5643afb3ad3b661ccb373eba8003f1383bd5709813fa1b01b2965a1951c",0,false},
{"e_body_throw_faults_job","f03cac8dd702de421a82b9d3166e1ad633ebc12116fe89e0fc414ecd7b8b4720",0,false},
{"e_child_throw_destroys_child_frame","3c165f30c42c00de2766842176db96ee300929ccec57ef325cb5b908f88877a6",0,false},
{"e_child_throw_faults_parent_once","5022132a705dff102d3a71a69866f8a9d051fd9c4f73c9118b2cd499a2e52c24",0,false},
{"e_exception_does_not_break_arena_accounting","6aae82b31cc3661076f09d8cce35229acaebec701c40c6d6dda70ec1b166db1c",0,false},
{"e_fault_releases_storage","f7a29269b93340f7204252877a85cb46ae7d2b48a05e44626cfc4fb4fdb88ccb",0,false},
{"e_fault_then_cancel_settles_once","0e5df2b0359942ccbf347ced0eea91fc9eaa529077a6e9d0fc7f4c1dc0285767",0,false},
{"e_job_may_handle_posted_fault","0cee3c53faae805380b735fc7a449d1940200c204469a8d79cd396c0e657d4c7",0,false},
{"f_cancel_one_does_not_affect_other","283b864fa6b9ea368dd7cd9db0b23346541505ed6aec04e7b2783c28bed3754a",0,false},
{"f_fault_one_does_not_affect_other","ce688968bb2ee6d5c377385e2af0854d0840637186197932612aa0613715cbbc",0,false},
{"f_interleaved_delivery_follows_delivery_order","3c293cd50793a153541ca498271b98dc86f70fbf48c43554c9acf5b2ef7fbec7",0,false},
{"f_two_jobs_progress_independently","c04be67ce43d93d27dbecd43461a035b6ccae69608e2e6bf74b53fca13d3aed4",0,false},
{"g_all_slots_returned_at_end","35fb6b0cc5dcc09057b73bc5990d4b8b70219a32820b0dfdfa20ebda58427f0b",0,false},
{"g_capacity_is_reused_not_grown","ce3414dec8b3533135a2aa2e97f6d3efd41b71012fc34658d0a040f420a94add",0,false},
{"g_exhaustion_is_reported","84d3efb7c62f68e4e9e35e17eb94a796c28376a75b6a022a75a6374e0072a848",0,false},
{"g_no_double_reclaims_across_workload","fddb448741a584fefe119e7dd97cb9a5f6386d8bfea4c20f78b8f65ef210e965",0,false},
{"g_peak_live_bounded_by_inflight","2104248d48df2157a3bad68fdc67e96e87e6619819fbf9fe3745b7302cf15166",0,false},
{"h_cancel_between_queue_and_delivery","4a06de535a9efd7dfbde4f85c0079a0dd656be2f5d581e6403136eb85ba7e0d6",0,false},
{"h_cancel_during_delivery_of_another_job","505761129d1eeabc4c2b96bad992ea6a9e52b34588e9cd6899a8808c546e82ea",0,false},
{"h_late_cancel_after_frame_reuse","151fb7820a14a8172c253813f0d9c673eab6dad3bd1e30e1a866e36dbbfb0739",0,false},
{"h_mixed_workload_invariants","338e9f2936dffc1ed5aa1cf3d3e62e503ecba6f9c006bb4abf0ce479fd4b271e",0,false},
{"h_self_cancel_from_inside_job","c827937e1551ad28b41abf941eb2b1d836e2f15d1cff2e96e256329902b503a2",0,false},
{"h_stale_ticket_arrives_after_two_reuses","9dea97d6c5ba313b6ce8ef951483f58db652a87473b76bcb0eb1bd4c81428a26",0,false},
{"h_two_jobs_swap_slots_repeatedly","ef730b8b944ed164159671c9243db9f176a8ab875c58f1f4323c654828a92451",0,false},
{"i_clock_moves_to_ready_time_and_never_back","01fcb4d6c3b10d8172b6fd64dd1d6a4c8a2376a7195e272ab1add9898ca9db98",0,false},
{"i_delay_is_measured_from_the_queueing_clock","e2a44e7612c3c2a8c8b13bcbb86a886e68ccd1844e2dba950294cffcc4fbf984",0,false},
{"i_delayed_fault_keeps_its_ready_time_and_its_frame","3faec8d9d7a889e513b69b9ed9ff6a6714516b2fc42f0fb066549916caa79354",0,false},
{"i_equal_ready_time_breaks_on_lowest_job_id","617943f8186e755c6c16bd99169918a89739dc0f2d0a0963bb2ac64b23ef6c0e",0,false},
{"i_ready_time_beats_queue_order","822373333105ae9f1e146306357f84323dc35824caf3857ffc654e3b35c278cc",0,false},
{"i_same_job_same_time_keeps_queue_order","2cd5904a29651e48795394b37ea25ffcec3b4f9959de3ccc56ce02ac4728bbba",0,false},
{"j_binding_survives_unrelated_reuse","376124a52ca1c3f9b9ff8d027e1bf5fd4375723294f96691f7c91f0a5bfad836",0,false},
{"j_same_index_new_generation_is_rejected","601d2c6d89ce33455c81b6eb50dae9d3014c7d786fd9836c06ab3ea5db9c52a5",0,false},
{"j_ticket_for_outer_frame_rejected_while_inner_parked","814362783513f95de5c5c62528ba9e3ba32fcd54c66aa3b20cc16e12b244dedf",0,false},
{"j_ticket_for_returned_child_rejected_after_reuse","37318f4792f51d3cc0aef150ff57916969a2a74cec4059022dd626d71de46c6c",0,false},
{"j_ticket_queued_before_launch_is_never_accepted","e0c26cee544b4a8127b5a7c3dfc070be0eb5d2c5e23766fd2e33410d79cc8b5e",0,false},
{"k_cancel_of_unknown_id_is_recorded_and_harmless","bf3fb4126a7c802dc5c95202894b5bcda27f6155167c4a3e0c0e9b38fb34f75b",0,false},
{"k_cancel_records_one_request_per_call","cb31d7830edaaa9a6eb147ae6b2c6b763005c7ce91cbd1798925a4a7750e511d",0,false},
{"l_compound_schedule_trace_is_exact","21b391dd96247767c8b63dce84fc48dcfaeca94b2ae5d7c5167ca2f470dbc36d",0,false},
{"l_equal_time_then_inner_frame_trace_is_exact","a470358451ee39756c44f2d289951ffa4f8a934f878520a4e472abe00d8aab93",0,false},
{"m_delay_from_resumed_clock_then_identifier_tie_trace","5ff4cea2ce4e2bac7df7470d2e524e2baac7103864c8b24f87d7868b9cb0ad16",0,false},
{"m_outer_ticket_rejected_while_inner_parked_trace","4530cb2168b8c7cf3a88788602a1ea92536dd1c3d4cecfd8cd9d2ecbe5151482",0,false},
{"m_self_cancel_then_slot_reuse_rejects_stale_trace","8e4c908a519a53614a60a5be8e018aba0b4518864eaafb88ccd86f95c3f02e27",0,false},
{"m_stale_ticket_across_two_generations_trace","f30b2648886fd4f942e6fcaf4834376db0475167f59841e55178d98e92bd7b93",0,false},
{"m_unbound_ticket_never_binds_after_later_park_trace","760707fef51a041630fd0b8872729fa83aca9c403b790e32061cc101a42db8a3",0,false},
{"zz_negative_control_must_fail","4f7c703ed5d8d11ddb83600f9d0e13aff4fc980af826eebf41387186dc463d16",1,true},
};

constexpr std::array<std::uint32_t, 64> kShaK = {
0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2};

std::uint32_t rotr(std::uint32_t x, unsigned n) { return (x >> n) | (x << (32 - n)); }

std::string sha256(const std::string& input) {
    std::vector<std::uint8_t> data(input.begin(), input.end());
    const std::uint64_t bits = static_cast<std::uint64_t>(data.size()) * 8;
    data.push_back(0x80);
    while ((data.size() % 64) != 56) data.push_back(0);
    for (int i = 7; i >= 0; --i) data.push_back(static_cast<std::uint8_t>(bits >> (i * 8)));
    std::array<std::uint32_t,8> h={0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    for (std::size_t off=0; off<data.size(); off+=64) {
        std::uint32_t w[64]{};
        for (int i=0;i<16;++i) w[i]=(std::uint32_t(data[off+4*i])<<24)|(std::uint32_t(data[off+4*i+1])<<16)|(std::uint32_t(data[off+4*i+2])<<8)|data[off+4*i+3];
        for (int i=16;i<64;++i) { auto s0=rotr(w[i-15],7)^rotr(w[i-15],18)^(w[i-15]>>3); auto s1=rotr(w[i-2],17)^rotr(w[i-2],19)^(w[i-2]>>10); w[i]=w[i-16]+s0+w[i-7]+s1; }
        auto a=h[0],b=h[1],c=h[2],d=h[3],e=h[4],f=h[5],g=h[6],hh=h[7];
        for (int i=0;i<64;++i) { auto s1=rotr(e,6)^rotr(e,11)^rotr(e,25); auto ch=(e&f)^((~e)&g); auto t1=hh+s1+ch+kShaK[i]+w[i]; auto s0=rotr(a,2)^rotr(a,13)^rotr(a,22); auto maj=(a&b)^(a&c)^(b&c); auto t2=s0+maj; hh=g;g=f;f=e;e=d+t1;d=c;c=b;b=a;a=t1+t2; }
        h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;h[4]+=e;h[5]+=f;h[6]+=g;h[7]+=hh;
    }
    std::ostringstream out; out<<std::hex<<std::setfill('0'); for(auto v:h) out<<std::setw(8)<<v; return out.str();
}

struct Run { int rc; std::string output; bool timeout; };

Run run_case(const char* binary, const char* name) {
    int fds[2]; if (pipe(fds)!=0) return {127,"",false};
    const pid_t pid=fork();
    if(pid==0){
        close(fds[0]); dup2(fds[1],STDOUT_FILENO); dup2(fds[1],STDERR_FILENO); close(fds[1]);
        if(geteuid()==0){ if(setgid(65534)!=0 || setuid(65534)!=0) _exit(126); }
        char* const child_argv[]={const_cast<char*>(binary),const_cast<char*>("--run"),const_cast<char*>(name),nullptr};
        char home[]="HOME=/tmp/roundel-candidate-home"; char path[]="PATH=/usr/bin:/bin";
#ifdef __APPLE__
        char asan[]="ASAN_OPTIONS=detect_leaks=0:abort_on_error=0:allocator_may_return_null=0:detect_stack_use_after_return=1";
#else
        char asan[]="ASAN_OPTIONS=detect_leaks=1:abort_on_error=0:allocator_may_return_null=0:detect_stack_use_after_return=1";
#endif
        char ubsan[]="UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1"; char lsan[]="LSAN_OPTIONS=report_objects=1";
        char* const child_env[]={home,path,asan,ubsan,lsan,nullptr}; execve(binary,child_argv,child_env); _exit(127);
    }
    close(fds[1]); fcntl(fds[0],F_SETFL,fcntl(fds[0],F_GETFL)|O_NONBLOCK);
    std::string output; bool done=false,timeout=false; int status=0; const auto until=std::chrono::steady_clock::now()+std::chrono::seconds(10);
    while(!done){
        char buf[4096]; for(;;){ssize_t n=read(fds[0],buf,sizeof(buf)); if(n>0){output.append(buf,static_cast<std::size_t>(n)); if(output.size()>1048576){kill(pid,SIGKILL);timeout=true;}} else break;}
        const pid_t w=waitpid(pid,&status,WNOHANG); if(w==pid) done=true;
        if(!done && (timeout || std::chrono::steady_clock::now()>until)){kill(pid,SIGKILL);waitpid(pid,&status,0);timeout=true;done=true;}
        if(!done) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    for(;;){char buf[4096];ssize_t n=read(fds[0],buf,sizeof(buf));if(n>0)output.append(buf,static_cast<std::size_t>(n));else break;} close(fds[0]);
    int rc=128; if(WIFEXITED(status)) rc=WEXITSTATUS(status); else if(WIFSIGNALED(status)) rc=128+WTERMSIG(status); return {rc,output,timeout};
}

} // namespace

int main(int argc,char** argv){
    if(argc!=3){std::cerr<<"usage: roundel_verifier CANDIDATE SUMMARY\n";return 2;}
    const char* binary=argv[1]; const char* summary=argv[2];
    int passed=0,graded=0,control_ok=0; std::ostringstream report;
    for(const auto& c:kCases){
        Run r=run_case(binary,c.name); const bool ok=!r.timeout && r.rc==c.expected_rc && sha256(r.output)==c.digest;
        if(c.control){control_ok=ok?1:0;report<<(ok?"CONTROL ok ":"CONTROL FAIL ")<<c.name<<"\n";}
        else{++graded;if(ok)++passed;report<<(ok?"PASS ":"FAIL ")<<c.name<<" rc="<<r.rc<<"\n";}
    }
    FILE* f=std::fopen(summary,"w"); if(!f){std::cerr<<"cannot write summary\n";return 2;}
    std::fprintf(f,"passed=%d\ngraded=%d\ncontrol_ok=%d\n",passed,graded,control_ok); const auto text=report.str(); std::fwrite(text.data(),1,text.size(),f); std::fclose(f);
    std::cout<<"verified="<<passed<<"/"<<graded<<" control="<<control_ok<<"\n";
    return passed==graded && control_ok==1 ? 0 : 1;
}

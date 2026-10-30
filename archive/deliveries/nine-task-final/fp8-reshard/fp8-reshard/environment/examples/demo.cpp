#include "fp8/state.hpp"
#include <iostream>
int main(){std::vector<fp8::SourceState>s{{"layer.a","tie.q",fp8::Role::Forward,0,0,0,64,16,{8,2,4},1}};std::vector<fp8::Target>t{{"layer.b","tie.q",fp8::Role::Forward,1,3,4,true}};std::cout<<fp8::bytes(fp8::restore(s,t))<<'\n';}
